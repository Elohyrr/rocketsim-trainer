#!/usr/bin/env python3
"""Télécharge des clips de prod et les convertit en chronologies lisibles (une ligne par décision).

Usage : convert.py --out DIR [--since UNIX|AAAA-MM-JJ] [--per-mechanic 5] [--max 60] [--exclude touch,bump,clear]
                   [--base URL] [--detector N] [--seed 1]
Écrit DIR/<id>.txt (chronologie) et DIR/manifest.json (liste des clips retenus).
Un clip = mécaniques ≤ per-mechanic : tous ; sinon échantillon aléatoire reproductible (seed).
Seul stdlib. Lancer avec `python3 -I`.
"""
import argparse, datetime, json, math, os, random, urllib.request, collections

def get(url):
    with urllib.request.urlopen(url, timeout=60) as r:
        return json.load(r)

def since_ts(s):
    if s is None: return 0
    if s.isdigit(): return int(s)
    return int(datetime.datetime.fromisoformat(s).replace(tzinfo=datetime.timezone.utc).timestamp())

def norm(v): return math.sqrt(sum(x * x for x in v))
def sub(a, b): return [x - y for x, y in zip(a, b)]
def dot(a, b): return sum(x * y for x, y in zip(a, b))
def cross(a, b): return [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]

def euler(fwd, up):
    """tangage (nez vers le haut = +), roulis (° autour de l'avant), cap (°)."""
    pitch = math.degrees(math.asin(max(-1, min(1, fwd[2]))))
    yaw = math.degrees(math.atan2(fwd[1], fwd[0]))
    right = cross(fwd, up)
    roll = math.degrees(math.asin(max(-1, min(1, -right[2]))))  # >0 : côté droit bas
    return pitch, roll, yaw, up[2]

def timeline(clip, traj):
    n = traj['frames']; dt = traj['dt']; ts = clip['tick_skip']
    hz = traj['hz']
    ag = traj['agent']; car = traj['cars'][ag]
    opp = traj['cars'][1 - ag] if len(traj['cars']) > 1 else None
    trig = traj.get('trigger_time')
    lines = []
    labels = ', '.join(f"{l['mechanic']}@{l['t']:.2f}s" for l in traj.get('labels', []))
    lines.append(f"CLIP {clip['id']} mécanique={clip['mechanic']} detector={clip.get('detector')} "
                 f"durée={traj['seconds']:.1f}s déclenchement={trig:.2f}s étiquettes=[{labels}] "
                 f"détail={json.dumps(clip.get('detail'))} qualité={json.dumps(clip.get('quality'))}")
    lines.append("Repère : z haut, uu ; but adverse de l'agent (bleu) = y positif (y≈+5120). v en uu/s. "
                 "pitch>0 nez en haut ; roll>0 côté droit bas ; cap en degrés ; up.z=1 à plat, <0 renversé.")
    lines.append("Flags : S=sol J=a sauté F=flip/dodge utilisé B=boost actif. ctl = gaz/dir/tang/lacet/roulis + bits (b boost, J saut, h frein à main) "
                 "tenus pendant la décision qui COMMENCE à ce t (steps ; 8 ticks). dist = distance voiture-balle (centres, uu). "
                 "dvB = variation de vitesse de la balle sur la dernière image (≥150 et dist<260 ⇒ toucher probable *T*).")
    lines.append("t     | car x y z | vit(h/v) | pitch roll cap upz | flags boost | ctl | balle x y z | vB dvB | dist | adv dist")
    steps = clip['steps']
    prev_bv = None
    for i in range(n):
        t = i * dt
        p = car['pose'][9*i:9*i+9]; pos = p[0:3]; fwd = p[3:6]; up = p[6:9]
        v = car['vel'][3*i:3*i+3] if car.get('vel') else [0, 0, 0]
        w = car['ang'][3*i:3*i+3] if car.get('ang') else [0, 0, 0]
        b = traj['ball'][9*i:9*i+3]
        bv = traj['ball_vel'][3*i:3*i+3] if traj.get('ball_vel') else [0, 0, 0]
        fl = car['flags'][i]
        fs = ('S' if fl & 1 else '-') + ('J' if fl & 4 else '-') + ('F' if fl & 8 else '-') + ('B' if fl & 2 else '-')
        pi, ro, ya, uz = euler(fwd, up)
        d = norm(sub(b, pos))
        dv = norm(sub(bv, prev_bv)) if prev_bv else 0
        prev_bv = bv
        tag = '*T*' if dv >= 150 and d < 260 else ''
        # contrôles : décision qui commence à t (steps indexés à 1/(120/ts) s)
        si = min(int(round(t / (ts / 120.0))), len(steps) - 1)
        c = steps[si][ag]; bits = c[5]
        cs = '{:+.1f}/{:+.1f}/{:+.1f}/{:+.1f}/{:+.1f}'.format(*c[:5]) + ('b' if bits & 1 else '') + ('J' if bits & 2 else '') + ('h' if bits & 4 else '')
        od = ''
        if opp:
            op = opp['pose'][9*i:9*i+3]; od = f"{norm(sub(op, pos)):.0f}"
        mark = ' <== DÉCLENCHEMENT' if trig is not None and abs(t - trig) < dt / 2 else ''
        lines.append(f"{t:5.2f} | {pos[0]:6.0f} {pos[1]:6.0f} {pos[2]:5.0f} | {math.hypot(v[0], v[1]):5.0f}/{v[2]:+5.0f} | "
                     f"{pi:+4.0f} {ro:+4.0f} {ya:+5.0f} {uz:+.2f} | {fs} {car['boost'][i]:3.0f} | {cs} | "
                     f"{b[0]:6.0f} {b[1]:6.0f} {b[2]:5.0f} | {norm(bv):5.0f} {dv:4.0f} | {d:4.0f} {tag} | {od}{mark}")
    return '\n'.join(lines) + '\n'

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--out', required=True)
    ap.add_argument('--base', default='https://rocketsim.celri.dev')
    ap.add_argument('--since', help="timestamp unix ou date AAAA-MM-JJ (created_unix ≥)")
    ap.add_argument('--per-mechanic', type=int, default=5)
    ap.add_argument('--exclude', default='touch,bump,clear', help='mécaniques ignorées (liste séparée par des virgules)')
    ap.add_argument('--max', type=int, default=60, help='plafond total de clips')
    ap.add_argument('--detector', type=int, help='garder seulement ce detector')
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--hz', type=int, default=15)
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    lst = get(a.base + '/clips')
    st = since_ts(a.since)
    ex = set(filter(None, a.exclude.split(',')))
    lst = [c for c in lst if c['mechanic'] not in ex and c['created_unix'] >= st and (a.detector is None or c.get('detector') == a.detector)]
    by = collections.defaultdict(list)
    for c in lst: by[c['mechanic']].append(c)
    rng = random.Random(a.seed)
    pick = []
    for m, cs in sorted(by.items()):
        pick += cs if len(cs) <= a.per_mechanic else rng.sample(cs, a.per_mechanic)
    if len(pick) > a.max:
        # plafond : on retire d'abord dans les mécaniques les plus représentées dans la sélection
        while len(pick) > a.max:
            cnt = collections.Counter(c['mechanic'] for c in pick)
            m = cnt.most_common(1)[0][0]
            pick.remove(rng.choice([c for c in pick if c['mechanic'] == m]))
    manifest = []
    for c in pick:
        dest = os.path.join(a.out, c['id'] + '.txt')
        if not os.path.exists(dest):
            try:
                full = get(f"{a.base}/clips/{c['id']}")
                traj = get(f"{a.base}/clips/{c['id']}/trajectory?hz={a.hz}")
            except Exception as e:  # clip sorti de l'anneau entre-temps, réseau…
                print(f"ignoré {c['id']} : {e}")
                continue
            full['detail'] = c.get('detail'); full['quality'] = c.get('quality'); full['detector'] = c.get('detector')
            open(dest, 'w').write(timeline(full, traj))
        manifest.append({'id': c['id'], 'mechanic': c['mechanic'], 'detector': c.get('detector'), 'created_unix': c['created_unix']})
    json.dump(manifest, open(os.path.join(a.out, 'manifest.json'), 'w'), indent=1)
    print(f"{len(manifest)} clips, {len(by)} mécaniques -> {a.out}")

if __name__ == '__main__':
    main()
