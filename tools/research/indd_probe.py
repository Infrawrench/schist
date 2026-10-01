#!/usr/bin/env python3
"""Read-only Phase 0 probe of the publicly documented INDD/XMP container.

This is not an object-stream decoder or a production codec. It reads only
explicit local document paths; it never downloads or executes anything.
Format sources and limitations: docs/indd-format.md.
"""
import argparse
import hashlib
import json
import struct
import xml.etree.ElementTree as ET
import zlib
from pathlib import Path

MASTER = bytes.fromhex('0606edf5d81d46e5bd31efe7fe74b71d')
HEAD = bytes.fromhex('de39397951884b6c8e63eef8aee0dd38')
TAIL = bytes.fromhex('fdcedb70f7864b4fa4d3c728b3417106')
MAX_FILE = 64 * 1024 * 1024


def probe(data):
    if not 8192 <= len(data) <= MAX_FILE:
        raise ValueError('probe accepts 8 KiB to 64 MiB documents')
    pages = [data[:4096], data[4096:8192]]
    if any(page[:16] != MASTER for page in pages):
        raise ValueError('invalid master-page signature')
    sequences = [struct.unpack_from('<Q', page, 264)[0] for page in pages]
    current = int(sequences[1] > sequences[0])
    master = pages[current]
    endian = {1: '<', 2: '>'}.get(master[24])
    if endian is None:
        raise ValueError('invalid stream byte order')
    count = struct.unpack_from('<I', master, 280)[0]
    offset = count * 4096
    if count < 2 or offset > len(data):
        raise ValueError('database extent exceeds document')
    result = dict(sha256=hashlib.sha256(data).hexdigest(), bytes=len(data),
                  master_sequences=sequences, active_master=current,
                  stream_endian='little' if endian == '<' else 'big',
                  database_pages=count, contiguous_start=offset, objects=[])
    while data[offset:offset + 16] == HEAD:
        if offset + 32 > len(data):
            raise ValueError('truncated object marker')
        uid, class_id, length, checksum = struct.unpack_from('<IIII', data, offset + 16)
        end = offset + 32 + length
        if end + 32 > len(data) or data[end:end + 16] != TAIL:
            raise ValueError('truncated object or invalid trailer')
        if data[offset + 16:offset + 24] != data[end + 16:end + 24]:
            raise ValueError('object identity differs in trailer')
        payload = data[offset + 32:end]
        obj = dict(offset=offset, uid=uid, class_id=hex(class_id), bytes=length,
                   checksum=hex(checksum), zlib_at=[])
        # Only test complete streams at two plausible starts, with a strict
        # expansion cap. Success says nothing about the decoded semantics.
        for start in (0, 4):
            try:
                decoder = zlib.decompressobj()
                decoded = decoder.decompress(payload[start:], 1024 * 1024 + 1)
                if decoder.eof and not decoder.unused_data and len(decoded) <= 1024 * 1024:
                    obj['zlib_at'].append(dict(offset=start, decoded_bytes=len(decoded)))
            except zlib.error:
                pass
        if payload[4:].startswith(b'<?xpacket begin='):
            xmp_length = struct.unpack_from(endian + 'I', payload)[0]
            if xmp_length > len(payload) - 4:
                raise ValueError('truncated XMP')
            root = ET.fromstring(payload[4:4 + xmp_length])
            obj['xmp_bytes'] = xmp_length
            obj['creator_tools'] = sorted({value for element in root.iter()
                for key, value in list(element.attrib.items()) + [(element.tag, element.text or '')]
                if key.endswith('}CreatorTool') and value})
        result['objects'].append(obj)
        offset = end + 32
    result['trailing_bytes'] = len(data) - offset
    result['trailing_nonzero_bytes'] = sum(byte != 0 for byte in data[offset:])
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('document', type=Path)
    args = parser.parse_args()
    if args.document.stat().st_size > MAX_FILE:
        parser.error('document exceeds probe limit')
    print(json.dumps(probe(args.document.read_bytes()), indent=2))
