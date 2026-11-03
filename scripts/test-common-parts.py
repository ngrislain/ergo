#!/usr/bin/env python3
"""Selection boundary, canonical aliases and reviewed source-data regressions."""
import importlib.util
import json
import pathlib
import unittest
import common_parts

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('coverage', ROOT/'scripts/check-common-parts.py')
assert spec is not None and spec.loader is not None
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class CommonParts(unittest.TestCase):
    def test_distinct_sets_strictly_greater_than_seven_and_alias_union(self):
        library = json.loads((ROOT/'crates/world/library/sources.lock.json').read_text())
        rows = [dict(set_num=f'set-{i}', part='983' if i % 2 else '3820.dat') for i in range(8)]
        rows += rows*5
        rows += [dict(set_num=f'set-{i}', part='unavailable.dat') for i in range(7)]
        result = audit.coverage(library, rows)
        self.assertEqual(result['common_parts'], 1)
        self.assertEqual(result['bundled'], 1)
        self.assertFalse(result['missing'])
        rows.append(dict(set_num='set-7', part='unavailable.dat'))
        self.assertEqual(audit.coverage(library, rows)['missing'], {'unavailable.dat': 8})

    def test_every_reviewed_common_candidate_is_bundled_with_source_geometry_and_usage(self):
        library = json.loads((ROOT/'crates/world/library/sources.lock.json').read_text())
        index = {alias: entry for id, entry in library['parts'].items() for alias in [id, *entry['catalogue']['aliases']]}
        for id in common_parts.CANDIDATES:
            entry = index[id]
            self.assertGreater(entry['catalogue']['set_count'], 7, id)
            self.assertTrue(entry['asset']['bytes'] > 0, id)
            self.assertTrue(entry['catalogue']['size'] or entry['catalogue'].get('nominal'), id)
            if not entry['catalogue']['size']:
                self.assertTrue(entry['catalogue']['connectors'], id)


if __name__ == '__main__': unittest.main()
