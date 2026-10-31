import argparse
import json
import re
import sys

SCALARS = (
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
)
NAMES = {
    1: 'double', 2: 'float', 3: 'int64', 4: 'uint64', 5: 'int32',
    6: 'fixed64', 7: 'fixed32', 8: 'bool', 9: 'string', 10: 'group',
    11: 'message', 12: 'bytes', 13: 'uint32', 14: 'enum', 15: 'sfixed32',
    16: 'sfixed64', 17: 'sint32', 18: 'sint64',
}


def varint(buf, at):
    acc = 0
    shift = 0
    while at < len(buf):
        byte = buf[at]
        at += 1
        acc |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return acc, at
        shift += 7
        if shift > 63:
            return None, at
    return None, at


def walk(buf):
    out = {}
    at = 0
    end = len(buf)
    while at < end:
        key, nxt = varint(buf, at)
        if key is None or key < 8 or key > 0xFFFFF:
            break
        field, wire = key >> 3, key & 7
        at = nxt
        if wire == 0:
            value, nxt = varint(buf, at)
            if value is None:
                break
            at = nxt
        elif wire == 2:
            size, nxt = varint(buf, at)
            if size is None or nxt + size > end:
                break
            value = buf[nxt:nxt + size]
            at = nxt + size
        elif wire == 5:
            if at + 4 > end:
                break
            value = buf[at:at + 4]
            at += 4
        elif wire == 1:
            if at + 8 > end:
                break
            value = buf[at:at + 8]
            at += 8
        else:
            break
        out.setdefault(field, []).append(value)
    return out


def text(node, key):
    values = node.get(key)
    if not values or not isinstance(values[0], (bytes, bytearray)):
        return None
    try:
        return values[0].decode('utf-8')
    except UnicodeDecodeError:
        return None


def field_node(raw):
    node = walk(raw)
    if 1 not in node or 3 not in node:
        return None
    name = text(node, 1)
    number = node[3][0]
    if not name or not isinstance(number, int):
        return None
    label = node[4][0] if 4 in node else 1
    kind = node[5][0] if 5 in node else 9
    resolved = NAMES.get(kind, 'string')
    type_name = text(node, 6)
    if type_name:
        resolved = type_name.lstrip('.')
    return {
        'name': name,
        'number': number,
        'type': resolved,
        'label': label,
        'repeated': label == 3,
    }


def enum_node(raw):
    node = walk(raw)
    name = text(node, 1)
    if not name:
        return None
    values = []
    for item in node.get(2, []):
        entry = walk(item)
        label = text(entry, 1)
        if label is None:
            continue
        values.append([label, entry[2][0] if 2 in entry else 0])
    return {'name': name, 'values': values}


def message_node(raw, depth=0):
    node = walk(raw)
    name = text(node, 1)
    if not name:
        return None
    nested = []
    if depth < 4:
        nested = [m for m in (message_node(x, depth + 1) for x in node.get(3, [])) if m]
    return {
        'name': name,
        'fields': [f for f in (field_node(x) for x in node.get(2, [])) if f],
        'nested': nested,
        'enums': [e for e in (enum_node(x) for x in node.get(4, [])) if e],
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('binary')
    parser.add_argument('-o', '--out', default='schema.json')
    args = parser.parse_args()

    data = open(args.binary, 'rb').read()
    header = re.compile(rb'\x0a[\x01-\x7f]((?:[a-zA-Z0-9_/]|\.)*\.proto)')
    found = {}

    for match in header.finditer(data):
        start = match.start()
        declared = match.group(1).decode()
        if len(declared) + 2 != match.end() - start:
            continue
        node = walk(data[start:start + 900000])
        name = text(node, 1)
        package = text(node, 2)
        if name != declared or not package:
            continue
        messages = [m for m in (message_node(x) for x in node.get(4, [])) if m]
        enums = [e for e in (enum_node(x) for x in node.get(5, [])) if e]
        if not messages and not enums:
            continue
        if name in found:
            continue
        found[name] = {
            'package': package,
            'deps': [text({'x': [d]}, 'x') for d in node.get(3, [])],
            'messages': messages,
            'enums': enums,
            'offset': start,
        }

    def count(nodes):
        total = 0
        for item in nodes:
            total += len(item['fields']) + count(item.get('nested', []))
        return total

    with open(args.out, 'w', encoding='utf-8') as handle:
        json.dump(found, handle, ensure_ascii=False, indent=1)

    messages = sum(len(v['messages']) for v in found.values())
    fields = sum(count(v['messages']) for v in found.values())
    enums = sum(len(v['enums']) + sum(len(m['enums']) for m in v['messages'])
                for v in found.values())
    print('files=%d messages=%d fields=%d enums=%d -> %s'
          % (len(found), messages, fields, enums, args.out))
    for name in sorted(found, key=lambda k: found[k]['offset']):
        entry = found[name]
        print('  %-32s messages=%-3d enums=%-2d'
              % (name, len(entry['messages']), len(entry['enums'])))
    return 0


if __name__ == '__main__':
    sys.exit(main())