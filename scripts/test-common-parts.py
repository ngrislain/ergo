#!/usr/bin/env python3
"""Selection boundary, canonical aliases and reviewed source-data regressions."""
import importlib.util
import json
import pathlib
import unittest
from typing import Any
import common_parts
import native_model
from build.landmark_revision import preserve_unchanged

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


class LandmarkRevision(unittest.TestCase):
    def test_duplicate_occurrences_retain_distinct_ids_and_frozen_records(self):
        import copy
        # Metadata-only fixture, not a claim of source geometry or calibration.
        def brick(id, x, step=0):
            return dict(id=id,part='3005.dat',color=71,rgb=0xa0a5a9,step=step,
                        position=[x,0,1],transform=[1,0,0,0,1,0,0,0,1])
        before: dict[str,Any]=dict(name='Accepted',geo=dict(latitude=0,longitude=0,arcseconds_per_block=1),
                    bricks=[brick(3,.500000001,4),brick(7,.500000001,8),brick(11,1.5)],
                    meshes={'3005.dat':dict(quality='source',fixture='accepted physical record')},
                    joints=[],fixed_bricks=[],groups=[],ground=[])
        desired=copy.deepcopy(before)
        desired['bricks']=[brick(1,.5),brick(2,.5),brick(3,2.5)]
        desired['meshes']['3005.dat']['fixture']='different recipe cache'
        source=dict(scene=before,assembly_id='fixture',base_revision='fixture-revision')
        document,report=preserve_unchanged(source,desired)
        self.assertEqual([piece['id'] for piece in document['pieces']],[3,7,12])
        self.assertEqual([piece['step'] for piece in document['pieces']],[4,8,0])
        self.assertEqual(document['meshes'],before['meshes'])
        self.assertEqual(document['model']['definitions']['landscape'][0]['transform'][0][3],.500000001)
        self.assertEqual((report['retained_unmodified_ids'],report['removed_occurrences'],report['added_occurrences']),(2,1,1))
        self.assertFalse(report['published'])
        desired['fixed_bricks']=[1]
        with self.assertRaisesRegex(ValueError,'metadata-preserving'):
            preserve_unchanged(source,desired)


if __name__ == '__main__': unittest.main()
