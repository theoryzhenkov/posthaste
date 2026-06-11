#!/usr/bin/env node
/**
 * Generates CSS keyframe data for the zero-inbox horse collision animation.
 *
 * Physics model (v3)
 * ──────────────────
 * Horse plows LEFT→RIGHT (snowplow, not billiard): every letter gets a
 * rightward primary impulse plus a random up/down kick.  Positions are
 * computed analytically with per-step exponential friction (no continuous
 * gravity — letters freeze at their scattered positions, like knocked-over
 * pins).  A left-to-right cascade pass then resolves overlapping letters by
 * bumping the right-side one further forward, giving the pile-up effect.
 *
 * Output per letter:
 *   delay       — ms when horse centre reaches that letter
 *   ax/ay/ar    — transform at 40 % of the CSS scatter animation (arc peak)
 *   px/py/pr    — transform at 100 % (final frozen position)
 *
 * Run: node apps/site/tools/gen-collision-keyframes.mjs
 */

// ── Seeded PRNG ───────────────────────────────────────────────────────────────
function mulberry32(seed) {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}
const rand = mulberry32(0xc0ffee42)
const randf = (lo, hi) => lo + rand() * (hi - lo)

// ── Letter layout ─────────────────────────────────────────────────────────────
const W = {
  A: 30,
  l: 12,
  ' ': 13,
  q: 26,
  u: 26,
  i: 10,
  e: 24,
  t: 17,
  '!': 10,
  Z: 29,
  r: 17,
  o: 26,
  n: 26,
  b: 26,
  x: 24,
}
const defaultW = 24
const LETTER_SPACING = -1.6 // letter-spacing: -0.04em at 40 px

const TEXT = 'All quiet! Zero inbox'
const chars = [...TEXT]

let cursor = 0
const rawLayout = chars.map((ch) => {
  const w = W[ch] ?? defaultW
  const cx = cursor + w / 2
  cursor += w + LETTER_SPACING
  return { ch, cx, w }
})
const totalWidth = cursor - LETTER_SPACING
const midX = totalWidth / 2
const layout = rawLayout.map((l) => ({ ...l, cx: l.cx - midX }))

// ── Horse kinematics ──────────────────────────────────────────────────────────
const HORSE_ENTRY = -380
const HORSE_PEAK = 220
const HORSE_SWEEP_MS = 1040 // 52 % of 2000 ms CSS animation
const HORSE_VX = (HORSE_PEAK - HORSE_ENTRY) / HORSE_SWEEP_MS // ≈ 0.577 px/ms

const tContact = layout.map(({ cx }) =>
  Math.max(0, Math.round((cx - HORSE_ENTRY) / HORSE_VX)),
)

// ── Friction-decay model ──────────────────────────────────────────────────────
// Each step (DT ms) velocity is multiplied by FRICTION.
// Total displacement = v0 * DT * Σ(FRICTION^i, i=0..N-1) = v0 * DT * (1-FRICTION^N)/(1-FRICTION)
const DT = 15
const FRICTION = 0.958 // 4.2 % per 15 ms step → stops in ~400 ms effective

const ARC_TIME = 300 // ms from contact → 40 % of 750 ms CSS animation
const FINAL_TIME = 750 // ms from contact → 100 %

function decayFactor(ms) {
  const n = Math.ceil(ms / DT)
  return ((1 - Math.pow(FRICTION, n)) / (1 - FRICTION)) * DT
}
const DF_ARC = decayFactor(ARC_TIME) // ≈ 210
const DF_FINAL = decayFactor(FINAL_TIME) // ≈ 327
const ARC_RATIO = DF_ARC / DF_FINAL // ≈ 0.64

// ── Impulse parameters ────────────────────────────────────────────────────────
// All letters get a rightward primary impulse (horse direction).
// Scatter magnitude depends on wave function (horse front hits centre hardest).
const VX_BASE = 0.38 // px/ms base rightward impulse
const VX_WAVE = 0.1 // extra for letters near text centre
const VX_JITTER = 0.12 // ±random
const VY_AMP = 0.14 // max up/down kick (no gravity — letters stay where they land)
const VY_BIAS = 0.01 // tiny downward bias (more letters go down than up)
const OMEGA_BASE = 0.06 // base rotation rate (deg/ms)
const OMEGA_VY_SCALE = 0.55 // rotation correlates with y direction

// ── Expected ranges ───────────────────────────────────────────────────────────
// px ∈ [(VX_BASE-VX_JITTER)*DF_FINAL, (VX_BASE+VX_WAVE+VX_JITTER)*DF_FINAL]
//    ≈ [85, 196] px  — all rightward ✓
// py ∈ [-VY_AMP*DF_FINAL, +VY_AMP*DF_FINAL] ≈ ±46 px  — up or down ✓
// pr ∈ ≈ ±(VY_AMP * OMEGA_VY_SCALE + OMEGA_BASE) * DF_FINAL ≈ ±45 deg ✓

// ── Compute initial scatter ───────────────────────────────────────────────────
const norm = layout.map(({ cx }) => cx / midX) // −1 (left) … +1 (right)

const scatter = layout.map((_, i) => {
  const wave = 1 - 0.35 * Math.pow(Math.abs(norm[i]), 0.5) // 1 at centre, 0.65 at edge
  const vx = VX_BASE + VX_WAVE * wave + randf(-VX_JITTER, VX_JITTER)
  const vy = randf(-VY_AMP, VY_AMP) + VY_BIAS
  const omega =
    Math.sign(vy) * OMEGA_BASE * wave + vy * OMEGA_VY_SCALE + randf(-0.02, 0.02)

  return {
    vx: Math.max(vx, 0.06), // never negative — horse only pushes forward
    vy,
    omega,
  }
})

// Initial absolute x in stage coords (used for cascade)
const absX = layout.map(({ cx }, i) => cx + scatter[i].vx * DF_FINAL)
const absY = layout.map((_, i) => scatter[i].vy * DF_FINAL)

// ── One-way cascade ───────────────────────────────────────────────────────────
// Process left→right.  If letter j would land within MIN_SPACING of letter i
// (same y-band), bump j further right.  This simulates letters pile-driving
// into each other without full elastic collision math.
const MIN_SPACING = 20 // px centre-to-centre in the same y-band
const BAND_WIDTH = 28 // px — y range that counts as "same band"
const CASCADE_CAP = 55 // px — max extra push to avoid rightmost letters flying off

// Track right-most used x per y-band slot
const bands = new Map() // key = rounded y-band centre → rightmost abs x

function bandKey(y) {
  return Math.round(y / BAND_WIDTH) * BAND_WIDTH
}

for (let i = 0; i < scatter.length; i++) {
  const bk = bandKey(absY[i])
  const prevRight = bands.get(bk) ?? -Infinity
  if (absX[i] < prevRight + MIN_SPACING) {
    const extra = Math.min(
      prevRight + MIN_SPACING - absX[i] + randf(2, 8),
      CASCADE_CAP,
    )
    absX[i] += extra
  }
  bands.set(bk, absX[i])
}

// ── Build output ──────────────────────────────────────────────────────────────
const ri = (n) => Math.round(n)
const r1 = (n) => Math.round(n * 10) / 10

console.log(
  '// Auto-generated by apps/site/tools/gen-collision-keyframes.mjs (v3)',
)
console.log('// prettier-ignore')
console.log('const LETTER_PHYSICS = [')
for (let i = 0; i < chars.length; i++) {
  const label = chars[i] === ' ' ? '·' : chars[i]
  const d = tContact[i]

  // Final positions
  const px = absX[i] - layout[i].cx // x displacement (always positive)
  const py = absY[i] // y displacement (neg=up, pos=down)
  const pr = scatter[i].omega * DF_FINAL

  // Arc positions (40 % of final — proportional for x, same decay factor for y/r)
  const ax = px * ARC_RATIO
  const ay = scatter[i].vy * DF_ARC
  const ar = scatter[i].omega * DF_ARC

  console.log(
    `  { delay:${String(d).padStart(4)},` +
      ` ax:${String(ri(ax)).padStart(5)}, ay:${String(ri(ay)).padStart(5)}, ar:${String(r1(ar)).padStart(7)},` +
      ` px:${String(ri(px)).padStart(5)}, py:${String(ri(py)).padStart(5)}, pr:${String(r1(pr)).padStart(7)} }, // ${label}`,
  )
}
console.log('] as const;')
console.log()
console.log(
  `// "${TEXT}" · stage ≈${Math.round(totalWidth)} px · horse ${HORSE_VX.toFixed(3)} px/ms`,
)
console.log(
  `// Delay ${tContact[0]}–${tContact[tContact.length - 1]} ms · px range ${Math.round(Math.min(...absX.map((x, i) => x - layout[i].cx)))}–${Math.round(Math.max(...absX.map((x, i) => x - layout[i].cx)))} px rightward`,
)
