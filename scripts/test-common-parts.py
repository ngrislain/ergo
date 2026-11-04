#!/usr/bin/env python3
"""Selection boundary, canonical aliases and reviewed source-data regressions."""
import importlib.util
import json
import pathlib
import unittest
import common_parts
import native_model

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('coverage', ROOT/'scripts/check-common-parts.py')
assert spec is not None and spec.loader is not None
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class CommonParts(unittest.TestCase):
    def test_current_construct_expansion_preserves_individually_posed_limbs(self):
        model=native_model.Model();model.construct('medieval-archer-v1',color=4)
        leaves=list(native_model.model_leaves(model.export()))
        parts=native_model.CONSTRUCTS['medieval-archer-v1']['parts']
        self.assertEqual(len(leaves),len(parts))
        for leaf,part in zip(leaves,parts):
            expected=native_model.instance('part',part['part'],part['position'],part.get('color'),part.get('rotation') or native_model.yaw(part.get('quarter_turns',0)*__import__('math').pi/2))
            self.assertEqual(leaf['transform'],expected['transform'])
        self.assertTrue(any(abs(leaf['transform'][0][2])>.01 for leaf in leaves))
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
                nominal = entry['catalogue']['nominal']
                self.assertTrue(entry['catalogue'].get('connectors') or nominal.get('cavity')
                                or nominal['studs_ldu'], id)


if __name__ == '__main__': unittest.main()
