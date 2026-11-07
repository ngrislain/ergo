"""Publish the committed synthesized part-library fixture for local tests.

The fixture is the reviewed tier plus a few derived parts, produced by the
part-library synthesis (`crates/assets/examples/synthesize.rs --fixture`).
"""
import os
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]
LIBRARY = ROOT / 'crates/world/tests/fixtures/part-library.json'
OBJECTS = ROOT / 'crates/assets/tests/fixtures/library-objects.cbor.gz'
# A closed port: MCP mirrors fall back to their verified local copy.
OFFLINE_ORIGIN = 'http://127.0.0.1:9'


def seed(store, server=ROOT / 'target/debug/ergo-server', environment=None):
    """Publish the fixture into a local object-store directory, never S3.

    The server loads `.env` from its working directory, so it runs inside the
    store directory (which has none) with every S3 variable removed.
    """
    store = pathlib.Path(store).resolve()
    store.mkdir(parents=True, exist_ok=True)
    assert not (store / '.env').exists(), 'Refusing to seed a directory with a .env file'
    environment = {k: v for k, v in (environment or os.environ).items()
                   if not k.startswith(('S3_', 'AWS_'))}
    environment['ERGO_DATA_DIR'] = str(store)
    result = subprocess.run([str(server), 'parts-publish', str(LIBRARY), str(OBJECTS)], cwd=store,
                            env=environment, check=True, capture_output=True, text=True)
    assert (store / 'catalog/part-library/current.json').exists(), result.stderr


def offline_mcp(environment, assets, server=ROOT / 'target/debug/ergo-server'):
    """Seed an MCP asset mirror and keep the MCP offline. Updates `environment`."""
    seed(assets, server, environment)
    environment['ERGO_MCP_ASSET_DIR'] = str(assets)
    environment['ERGO_API_URL'] = OFFLINE_ORIGIN
    return environment
