// Vérifie la caméra voiture du lecteur sur de vrais clips : node camera-test.mjs [base-url] [mécanique…]
// Sans réseau : lit les fichiers JSON passés en argument (--file x.json).
import fs from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert';
const html = fs.readFileSync(new URL('./index.html', import.meta.url), 'utf8');
const src = html.slice(html.indexOf('// CAMCORE-BEGIN'), html.indexOf('// CAMCORE-END'));
const C = vm.runInNewContext(src + ';({CAM,camStep,camVFov,camReset})');
const base = process.argv[2] || 'https://rocketsim.celri.dev';
const mechs = process.argv.slice(3).length ? process.argv.slice(3) : ['tornado_spin', 'recovery', 'air_roll_shot', 'half_flip', 'fast_aerial'];
const list = await (await fetch(`${base}/clips`)).json();
const P = (a, o) => [a[o], a[o + 2], -a[o + 1]]; // repère RL -> three.js
let fail = 0;
for (const m of mechs) {
  const meta = list.find(c => c.mechanic === m); if (!meta) { console.log(m, ': absent'); continue; }
  const t = await (await fetch(`${base}/clips/${meta.id}/trajectory?hz=30`)).json();
  const c = t.cars[t.agent], st = {}; let prev = null, maxJump = 0, minBehind = 1, maxYawRate = 0, flipped = 0, maxUpTilt = 0;
  for (let i = 0; i < t.frames; i++) {
    const j = Math.min(i + 1, t.frames - 1);
    const pos = P(c.pose, i * 9), fwd = P(c.pose, i * 9 + 3), up = P(c.pose, i * 9 + 6);
    const vel = [0, 1, 2].map(k => (P(c.pose, j * 9)[k] - pos[k]) / t.dt);
    const r = C.camStep(st, pos, vel, fwd, i ? t.dt : 0);
    // la caméra est derrière la voiture selon le cap lissé (jamais de l'autre côté du cap) et à hauteur constante
    const dx = pos[0] - r.pos[0], dz = pos[2] - r.pos[2], n = Math.hypot(dx, dz);
    minBehind = Math.min(minBehind, (dx * r.look[0] + dz * r.look[2]) / n / Math.hypot(r.look[0], r.look[2]));
    if (up[1] < 0) flipped++;
    if (prev) {
      maxJump = Math.max(maxJump, Math.hypot(r.pos[0] - prev.pos[0], r.pos[2] - prev.pos[2], r.pos[1] - prev.pos[1]) - Math.hypot(...[0, 1, 2].map(k => pos[k] - prev.car[k])));
      let d = r.yaw - prev.yaw; maxYawRate = Math.max(maxYawRate, Math.abs(d) / t.dt);
    }
    prev = { pos: r.pos, yaw: r.yaw, car: pos };
  }
  const ok = minBehind > 0.99 && maxYawRate < 7 && maxJump < 150;
  if (!ok) fail++;
  console.log(`${ok ? 'OK ' : 'KO '} ${m}: ${t.frames} images, voiture à l'envers ${flipped}, caméra derrière le cap ≥ ${minBehind.toFixed(3)}, ` +
    `rotation max du cap ${(maxYawRate * 180 / Math.PI).toFixed(0)}°/s, saut caméra relatif max ${maxJump.toFixed(0)} uu/image`);
}
// Invariance : la caméra ne dépend ni du roulis ni du tangage (seule la vitesse/l'avant à plat comptent)
const a = {}, b = {};
const ra = C.camStep(a, [0, 500, 0], [1000, 0, 0], [1, 0, 0], 0), rb = C.camStep(b, [0, 500, 0], [1000, 0, 0], [0, -1, 0.001], 0);
assert.deepStrictEqual(ra.pos.map(Math.round), rb.pos.map(Math.round));
assert(Math.abs(ra.pos[0] + C.CAM.dist) < 1e-6 && ra.pos[1] === 500 + C.CAM.height);
console.log('invariance orientation : OK');
process.exit(fail ? 1 : 0);
