"""Independent one-track fixtures packed from the official tagcache C structs."""
import pathlib
import struct

root = pathlib.Path(__file__).parent
for version in (15, 16):
    for endian in ("le", "be"):
        folder = root / f"v{version}-{endian}"
        folder.mkdir(exist_ok=True)
        pack = lambda *values: struct.pack(("<" if endian == "le" else ">") + "i" * len(values), *values)
        strings = {0: "Artist", 1: "Album", 2: "Rock", 3: "Zèbre", 4: "/Music/song.mp3", 5: "Composer", 6: "Comment", 7: "Album Artist", 8: "Group"}
        if version == 16:
            strings[12] = "Artist"
        values = [0] * (22 if version == 15 else 23)
        payload_bytes = 0
        for tag, text in strings.items():
            value = text.encode() + b"\0"
            value += b"X" * (-len(value) % 8)
            data = pack(len(value), 0 if tag in (3,4) else -1) + value
            (folder / f"database_{tag}.tcd").write_bytes(pack(0x54434800 + version, len(data), 1) + data)
            values[tag] = 12
            if tag != 4:
                payload_bytes += len(data)
        shift = 1 if version == 15 else 0
        values[9:12] = [2001,2,3]
        values[13-shift:] = [192, 123456, 12, 7, 765432, 44, 9, 100, 1500, 4500]
        size = 24 + (len(values)+1)*4 + payload_bytes
        (folder / "database_idx.tcd").write_bytes(pack(0x54434800 + version, size, 1, 44, 9, 0, *values, 0))
