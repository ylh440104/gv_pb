#!/usr/bin/env python3
"""
extract_rtti.py - Recover C++ RTTI class/namespace names from a PE image.

MSVC emits RTTI type descriptors as ".?AV<name>@<namespace>@@". This tool pulls
every one out and converts the MSVC mangling into C++ "ns::Class" form.

Usage:
    python3 extract_rtti.py <binary.exe> [-o out.txt]
"""
import re
import sys
import argparse

PAT = re.compile(rb'\.\?A[VU]([^\x00]{2,240}?)@@')


def demangle(raw: bytes) -> str:
    s = raw.decode('latin1')
    s = s.replace('@@', '')
    s = s.replace('@', '::')
    return s


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument('binary')
    ap.add_argument('-o', '--out', default=None)
    a = ap.parse_args()

    data = open(a.binary, 'rb').read()
    names = set()
    for m in PAT.finditer(data):
        names.add(demangle(m.group(1)))

    out = '\n'.join(sorted(names))
    if a.out:
        open(a.out, 'w').write(out + '\n')
        print('wrote %d symbols to %s' % (len(names), a.out))
    else:
        print(out)
    print('total: %d' % len(names), file=sys.stderr)
    return 0


if __name__ == '__main__':
    sys.exit(main())