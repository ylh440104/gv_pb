#!/usr/bin/env python3
"""
gen_proto.py - Emit compilable .proto files from an extracted gv_pb schema.

Input : schema.json produced by extract_proto.py
Output: one .proto per logical unit inside <outdir>

connect.proto and general.proto import each other in the real binary. A
generated descriptor set cannot express an import cycle, so those two are
merged into a single "session.proto" unit. Everything else keeps its original
file name.

Usage:
    python3 gen_proto.py schema.json -o proto/
"""
import json
import os
import sys
import argparse

SCALARS = {
    'double', 'float', 'int64', 'uint64', 'int32', 'fixed64', 'fixed32',
    'bool', 'string', 'bytes', 'uint32', 'sfixed32', 'sfixed64',
    'sint32', 'sint64',
}

ORDER = [
    'defines.proto', 'feature_flag.proto', 'connect.proto', 'general.proto',
    'input_event.proto', 'notification.proto', 'clipboard.proto', 'draw.proto',
    'port_mapping.proto', 'rpc_message.proto', 'rpc_main.proto', 'mumu.proto',
    'rpc_file_transfer.proto', 'main.proto',
]

GROUP = {'connect.proto': 'session.proto', 'general.proto': 'session.proto'}


def register(msgs, fname, owner):
    for m in msgs:
        owner[m['name']] = fname
        register(m.get('nested', []), fname, owner)
        for e in m.get('enums', []):
            owner[e['name']] = fname


def collect_refs(msgs, acc):
    for m in msgs:
        for f in m['fields']:
            if f['type'] not in SCALARS:
                acc.add(f['type'].split('.')[-1])
        collect_refs(m.get('nested', []), acc)


def render_field(f):
    t = f['type']
    tn = t if t in SCALARS else t.split('.')[-1]
    prefix = 'repeated ' if f.get('repeated') else ''
    return '  %s%s %s = %d;' % (prefix, tn, f['name'], f['num'])


def emit_message(m, indent=''):
    L = [indent + 'message %s {' % m['name']]
    for e in m.get('enums', []):
        L.append('%s  enum %s {' % (indent, e['name']))
        for n, v in e['values']:
            L.append('%s    %s = %d;' % (indent, n, v))
        L.append('%s  }' % indent)
    for n in m.get('nested', []):
        L += emit_message(n, indent + '  ')
    for f in m['fields']:
        L.append(indent + render_field(f))
    L.append(indent + '}')
    return L


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('schema')
    ap.add_argument('-o', '--outdir', default='proto')
    a = ap.parse_args()

    S = json.load(open(a.schema))
    order = [f for f in ORDER if f in S]
    os.makedirs(a.outdir, exist_ok=True)

    owner = {}
    for fn in order:
        register(S[fn]['messages'], fn, owner)
        for e in S[fn]['enums']:
            owner[e['name']] = fn

    units = {}
    for fn in order:
        units.setdefault(GROUP.get(fn, fn), []).append(fn)

    for unit, fns in units.items():
        acc = set()
        for fn in fns:
            collect_refs(S[fn]['messages'], acc)
        need = sorted({
            GROUP.get(owner[x], owner[x])
            for x in acc
            if x in owner and GROUP.get(owner[x], owner[x]) != unit
        })
        L = ['syntax = "proto3";', '']
        for d in need:
            L.append('import "%s";' % d)
        if need:
            L.append('')
        L += ['package gv_pb;', '']
        for fn in fns:
            for e in S[fn]['enums']:
                L.append('enum %s {' % e['name'])
                for n, v in e['values']:
                    L.append('  %s = %d;' % (n, v))
                L += ['}', '']
            for m in S[fn]['messages']:
                L += emit_message(m)
                L.append('')
        p = os.path.join(a.outdir, unit)
        open(p, 'w').write('\n'.join(L))
        print('wrote %s (%d lines)' % (p, len(L)))
    return 0


if __name__ == '__main__':
    sys.exit(main())