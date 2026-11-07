"""Publish the committed synthesized part-library fixture for local tests.

The fixture is the reviewed tier plus a few derived parts, produced by the
part-library synthesis (`crates/assets/examples/synthesize.rs --fixture`).
Seeding writes the published store layout directly (pointer, library, mesh
objects), so it needs no server binary and can never reach a configured S3
bucket through a `.env` file.
"""
import gzip
import hashlib
import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
LIBRARY = ROOT / 'crates/world/tests/fixtures/part-library.json'
OBJECTS = ROOT / 'crates/assets/tests/fixtures/library-objects.cbor.gz'
# A closed port: MCP mirrors fall back to their verified local copy.
OFFLINE_ORIGIN = 'http://127.0.0.1:9'


def _cbor_map(data):
    """Decode the fixture's CBOR map of text mesh hashes to byte strings."""
    position = 0

    def head():
        nonlocal position
        initial = data[position]
        position += 1
        major, info = initial >> 5, initial & 31
        if info < 24:
            return major, info
        size = {24: 1, 25: 2, 26: 4, 27: 8}[info]
        value = int.from_bytes(data[position:position + size], 'big')
        position += size
        return major, value

    def string(expected):
        nonlocal position
        major, length = head()
        assert major == expected, 'Unexpected CBOR item in the object fixture'
        value = data[position:position + length]
        position += length
        return value

    major, count = head()
    assert major == 5, 'The object fixture must be a CBOR map'
    return {string(3).decode(): string(2) for _ in range(count)}


def seed(store):
    """Publish the fixture into a local object-store directory."""
    store = pathlib.Path(store).resolve()
    library = json.loads(LIBRARY.read_text())
    revision = library['revision']
    objects = _cbor_map(gzip.decompress(OBJECTS.read_bytes()))
    for entry in library['parts'].values():
        data = objects[entry['mesh_hash']]
        assert hashlib.sha256(data).hexdigest() == entry['mesh_hash'], 'Corrupt fixture object'
        path = store / 'objects' / f"{entry['mesh_hash']}.cbor"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    document = store / 'catalog/part-library' / revision / 'library.json.gz'
    document.parent.mkdir(parents=True, exist_ok=True)
    document.write_bytes(gzip.compress(LIBRARY.read_bytes(), mtime=0))
    pointer = {'format': 'ergo-part-library-pointer-1', 'revision': revision}
    (store / 'catalog/part-library/current.json').write_text(json.dumps(pointer))


def offline_mcp(environment, assets):
    """Seed an MCP asset mirror and keep the MCP offline. Updates `environment`."""
    seed(assets)
    environment['ERGO_MCP_ASSET_DIR'] = str(assets)
    environment['ERGO_API_URL'] = OFFLINE_ORIGIN
    return environment
