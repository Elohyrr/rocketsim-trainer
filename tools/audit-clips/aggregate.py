#!/usr/bin/env python3
"""Agrège les verdicts des juges (fichiers *.jsonl) en tableau Markdown par mécanique.

Usage : aggregate.py VERDICTS_DIR   (lit DIR/*.jsonl, une ligne JSON par clip : id, mechanic, verdict, raison, piege)
Sortie : tableau trié par taux de faux décroissant, puis exemples de clips faux (3 par mécanique).
"""
import collections, glob, json, os, sys

def main():
    rows = {}
    for f in sorted(glob.glob(os.path.join(sys.argv[1], '*.jsonl'))):
        for line in open(f):
            line = line.strip()
            if line.startswith('{'):
                try:
                    r = json.loads(line); rows[r['id']] = r
                except Exception:
                    pass
    by = collections.defaultdict(list)
    for r in rows.values(): by[r['mechanic']].append(r)
    stats = []
    for m, rs in by.items():
        c = collections.Counter(r['verdict'] for r in rs)
        stats.append((m, len(rs), c['vrai'], c['faux'], c['douteux']))
    stats.sort(key=lambda s: (-(s[3] / s[1]), -s[1]))
    print('| Mécanique | jugés | vrais | faux | douteux | taux de faux |\n|---|---|---|---|---|---|')
    tot = [0, 0, 0, 0]
    for m, n, v, f, d in stats:
        print(f'| {m} | {n} | {v} | {f} | {d} | {100*f/n:.0f} % |')
        for i, x in enumerate((n, v, f, d)): tot[i] += x
    print(f'| **total** | {tot[0]} | {tot[1]} | {tot[2]} | {tot[3]} | {100*tot[2]/max(tot[0],1):.0f} % |')
    print()
    for m, *_ in stats:
        fx = [r for r in by[m] if r['verdict'] == 'faux'][:3]
        for r in fx:
            print(f"- `{r['id']}` ({m}) : {r['raison']} — piège : {r.get('piege','')}")

if __name__ == '__main__':
    main()
