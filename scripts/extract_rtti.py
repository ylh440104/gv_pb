import argparse
import re
import sys

PATTERN = re.compile(rb'\.\?A[VU]([^\x00]{2,240}?)@@')


def convert(raw):
    name = raw.decode('latin1')
    return name.replace('@@', '').replace('@', '::')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('binary')
    parser.add_argument('-o', '--out')
    args = parser.parse_args()

    data = open(args.binary, 'rb').read()
    names = sorted({convert(m.group(1)) for m in PATTERN.finditer(data)})
    body = '\n'.join(names) + '\n'

    if args.out:
        open(args.out, 'w', encoding='utf-8').write(body)
        print('wrote %d symbols to %s' % (len(names), args.out), file=sys.stderr)
    else:
        sys.stdout.write(body)
        print('total %d' % len(names), file=sys.stderr)
    return 0


if __name__ == '__main__':
    sys.exit(main())