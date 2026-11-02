#!/usr/bin/env python3
"""Audit distinct-set part coverage from a normalized LDraw inventory CSV.

Input columns: set_num, part (official LDraw ID, with or without .dat).
Repeated colours, spare entries and quantities in one set count only once.
Without an input CSV, report the bundled library's recorded catalogue usage.
"""
import argparse
import csv
import json
import pathlib
from collections import defaultdict

ROOT = pathlib.Path(__file__).resolve().parents[1]


def coverage(library, rows=None, threshold=7):
    entries = {id: entry['catalogue'] for id, entry in library['parts'].items()}
    aliases = {alias: id for id, entry in entries.items() for alias in [id, *entry['aliases']]}
    if rows is None:
        counts = {id: entry['set_count'] for id, entry in entries.items() if entry.get('set_count') is not None}
    else:
        sets = defaultdict(set)
        for row in rows:
            id = row['part'].strip().lower()
            if not id.endswith('.dat'): id += '.dat'
            if not row['set_num'].strip(): raise ValueError('Empty set identifier')
            sets[aliases.get(id, id)].add(row['set_num'].strip())
        counts = {id: len(values) for id, values in sets.items()}
    selected = {id: count for id, count in counts.items() if count > threshold}
    missing = {id: count for id, count in selected.items() if id not in entries}
    no_interfaces = {id: count for id, count in selected.items() if id in entries
                     and entries[id]['size'] is None and not entries[id].get('nominal') and not entries[id].get('profile_id')}
    return dict(scope='supplied distinct-set inventory' if rows is not None else 'recorded bundled catalogue items (not the complete LEGO catalogue)',
                threshold_exclusive=threshold, common_parts=len(selected), bundled=len(selected)-len(missing),
                missing=missing, missing_profiles=no_interfaces,
                parts_without_recorded_usage=[id for id, entry in entries.items() if entry.get('set_count') is None])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('inventory', nargs='?', type=pathlib.Path)
    parser.add_argument('--threshold', type=int, default=7)
    parser.add_argument('--require-complete', action='store_true')
    args = parser.parse_args()
    library = json.loads((ROOT/'crates/world/library/sources.lock.json').read_text())
    if args.inventory:
        with args.inventory.open(newline='') as stream:
            result = coverage(library, csv.DictReader(stream), args.threshold)
    else: result = coverage(library, threshold=args.threshold)
    print(json.dumps(result, indent=2, sort_keys=True))
    if args.require_complete and (result['missing'] or result['missing_profiles']): raise SystemExit(1)


if __name__ == '__main__': main()
