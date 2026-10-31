#!/usr/bin/env python3
"""
extract_proto.py - Recover embedded protobuf FileDescriptorProto sets from a PE.

MSVC-backed protobuf builds embed the compiled descriptor tables in .rdata in a
form that still contains the original length-delimited FileDescriptorProto
bytes (file name, package, dependency list, message/field/enum definitions all
as raw wire format). This tool scans for FileDescriptorProto headers, parses
them leniently, and writes the whole set to schema.json.

No symbol table, no PDB, and no running process are required.

Usage:
    python3 extract_proto.py <binary.exe> [-o schema.json]
"""
import json
import os
import re
import sys
import argparse

TYPE = {
    1: 'double', 2: 'float', 3: 'int64', 4: 'uint64', 5: 'int32',
    6: 'fixed64', 7: 'fixed32', 8: 'bool', 9: 'string', 10: 'group',
    11: 'message', 12: 'bytes', 13: 'uint32', 14: 'enum', 15: 'sfixed32',
    16: 'sfixed64', 17: 'sint32', 18: 'sint64',
}


def read_varint(b, i):
    r = 0
    s = 0
    while i < len(b):
        c = b[i]
        i += 1
        r |= (c & 0x7f) << s
        if not c & 0x80:
            return r, i
        s += 7
        if s > 63:
            return None, i
    return None, i


def parse(b):
    """Lenient wire-format parse; stops at the first structural error."""
    out = {}
    i = 0
    n = len(b)
    while i < n:
        key, i2 = read_varint(b, i)
        if key is None or key < 8 or key > 0xFFFFF:
            break
        fnum, wt = key >> 3, key & 7
        i = i2
        if wt == 0:
            v, i3 = read_varint(b, i)
            if v is None:
                break
            i = i3
        elif wt == 2:
            ln, i3 = read_varint(b, i)
            if ln is None or i3 + ln > n:
                break
            v = b[i3:i3 + ln]
            i = i3 + ln
        elif wt == 5:
            if i + 4 > n:
                break
            v = b[i:i + 4]
            i += 4
        elif wt == 1:
            if i + 8 > n:
                break
            v = b[i:i + 8]
            i += 8
        else:
            break
        out.setdefault(fnum, []).append(v)
    return out


def as_str(f, k):
    v = f.get(k)
    if not v or not isinstance(v[0], bytes):
        return None
    try:
        return v[0].decode('utf-8')
    except Exception:
        return None


def field_def(buf):
    f = parse(buf)
    if 1 not in f or 3 not in f:
        return None
    name = as_str(f, 1)
    num = f[3][0]
    if not name or not isinstance(num, int):
        return None
    label = f[4][0] if 4 in f else 1
    typ = f[5][0] if 5 in f else 9
    t = TYPE.get(typ, 'string')
    tn = as_str(f, 6)
    if tn:
        t = tn.lstrip('.')
    return {
        'name': name, 'num': num, 'type': t,
        'label': label, 'raw_type': typ, 'repeated': label == 3,
    }


def enum_def(buf):
    f = parse(buf)
    name = as_str(f, 1)
    if not name:
        return None
    vals = []
    for v in f.get(2, []):
        vf = parse(v)
        vn = as_str(vf, 1)
        if vn is None:
            continue
        vals.append([vn, vf[2][0] if 2 in vf else 0])
    return {'name': name, 'values': vals}


def message_def(buf, depth=0):
    f = parse(buf)
    name = as_str(f, 1)
    if not name:
        return None
    nested = []
    if depth < 4:
        nested = [x for x in (message_def(y, depth + 1) for y in f.get(3, [])) if x]
    return {
        'name': name,
        'fields': [x for x in (field_def(y) for y in f.get(2, [])) if x],
        'nested': nested,
        'enums': [x for x in (enum_def(y) for y in f.get(4, [])) if x],
        'oneofs': [as_str(parse(x), 1) for x in f.get(8, [])],
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('binary')
    ap.add_argument('-o', '--out', default='schema.json')
    a = ap.parse_args()

    raw = open(a.binary, 'rb').read()
    results = {}
    hdr = re.compile(rb'\x0a[\x01-\x7f]((?:[a-zA-Z0-9_/]|\.)*\.proto)')

    for m in hdr.finditer(raw):
        start = m.start()
        name0 = m.group(1).decode()
        if len(name0) + 2 != m.end() - start:
            continue
        f = parse(raw[start:start + 900000])
        name = as_str(f, 1)
        pkg = as_str(f, 2)
        if name != name0 or not pkg:
            continue
        msgs = [x for x in (message_def(y) for y in f.get(4, [])) if x]
        enums = [x for x in (enum_def(y) for y in f.get(5, [])) if x]
        if not msgs and not enums:
            continue
        if name in results:
            continue
        results[name] = {
            'package': pkg,
            'deps': [as_str({'x': [d]}, 'x') for d in f.get(3, [])],
            'messages': msgs,
            'enums': enums,
            'offset': start,
        }

    def count_fields(ms):
        n = 0
        for x in ms:
            n += len(x['fields']) + count_fields(x.get('nested', []))
        return n

    total_msgs = sum(len(v['messages']) for v in results.values())
    total_fields = sum(count_fields(v['messages']) for v in results.values())
    total_enums = sum(len(v['enums']) + sum(len(x['enums']) for x in v['messages'])
                      for v in results.values())

    json.dump(results, open(a.out, 'w'), ensure_ascii=False, indent=1)
    print('files=%d messages=%d fields=%d enums=%d -> %s'
          % (len(results), total_msgs, total_fields, total_enums, a.out))
    for k in sorted(results, key=lambda x: results[x]['offset']):
        v = results[k]
        print('  %-32s msgs=%-3d enums=%-2d' % (k, len(v['messages']), len(v['enums'])))
    return 0


if __name__ == '__main__':
    sys.exit(main())