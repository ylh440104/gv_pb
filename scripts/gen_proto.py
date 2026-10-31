import argparse
import json
import os
import sys

SCALARS = frozenset([
    'double', 'float', 'int64', 'uint64', 'int32', 'fixed64', 'fixed32',
    'bool', 'string', 'bytes', 'uint32', 'sfixed32', 'sfixed64',
    'sint32', 'sint64',
])

SOURCES = [
    'defines.proto', 'feature_flag.proto', 'connect.proto', 'general.proto',
    'input_event.proto', 'notification.proto', 'clipboard.proto', 'draw.proto',
    'port_mapping.proto', 'rpc_message.proto', 'rpc_main.proto', 'mumu.proto',
    'rpc_file_transfer.proto', 'main.proto',
]

MERGED = {'connect.proto': 'session.proto', 'general.proto': 'session.proto'}


def collect_owners(messages, source, owners):
    for item in messages:
        owners[item['name']] = source
        collect_owners(item.get('nested', []), source, owners)
        for entry in item.get('enums', []):
            owners[entry['name']] = source


def collect_references(messages, seen):
    for item in messages:
        for field in item['fields']:
            if field['type'] not in SCALARS:
                seen.add(field['type'].split('.')[-1])
        collect_references(item.get('nested', []), seen)


def render_field(field, indent=''):
    kind = field['type']
    if kind not in SCALARS:
        kind = kind.split('.')[-1]
    prefix = 'repeated ' if field.get('repeated') else ''
    return '%s%s%s %s = %d;' % (indent, prefix, kind, field['name'], field['num'])


def render_message(item, indent=''):
    lines = [indent + 'message %s {' % item['name']]
    for entry in item.get('enums', []):
        lines.append(indent + '  enum %s {' % entry['name'])
        for label, number in entry['values']:
            lines.append(indent + '    %s = %d;' % (label, number))
        lines.append(indent + '  }')
    for nested in item.get('nested', []):
        lines.extend(render_message(nested, indent + '  '))
    for field in item['fields']:
        lines.append(render_field(field, indent + '  '))
    lines.append(indent + '}')
    return lines


def render_enum(entry):
    lines = ['enum %s {' % entry['name']]
    for label, number in entry['values']:
        lines.append('  %s = %d;' % (label, number))
    lines.append('}')
    return lines


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('schema')
    parser.add_argument('-o', '--outdir', default='proto')
    args = parser.parse_args()

    schema = json.load(open(args.schema, encoding='utf-8'))
    sources = [name for name in SOURCES if name in schema]
    os.makedirs(args.outdir, exist_ok=True)

    owners = {}
    for source in sources:
        collect_owners(schema[source]['messages'], source, owners)
        for entry in schema[source]['enums']:
            owners[entry['name']] = source

    units = {}
    for source in sources:
        units.setdefault(MERGED.get(source, source), []).append(source)

    for unit, members in units.items():
        referenced = set()
        for source in members:
            collect_references(schema[source]['messages'], referenced)
        imports = set()
        for name in referenced:
            origin = owners.get(name)
            if origin is None:
                continue
            origin_unit = MERGED.get(origin, origin)
            if origin_unit != unit:
                imports.add(origin_unit)

        lines = ['syntax = "proto3";', '']
        for dep in sorted(imports):
            lines.append('import "%s";' % dep)
        if imports:
            lines.append('')
        lines.append('package gv_pb;')
        lines.append('')

        for source in members:
            for entry in schema[source]['enums']:
                lines.extend(render_enum(entry))
                lines.append('')
            for item in schema[source]['messages']:
                lines.extend(render_message(item))
                lines.append('')

        with open(os.path.join(args.outdir, unit), 'w', encoding='utf-8') as handle:
            handle.write('\n'.join(lines))
        print('%s <- %s' % (unit, ', '.join(members)))
    return 0


if __name__ == '__main__':
    sys.exit(main())