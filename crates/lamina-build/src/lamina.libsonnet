// Helpers for Lamina design sources.
//
//   local lamina = import 'lamina.libsonnet';
//
// A design source evaluates to an object with one or more of the keys
// `board`, `parts` and `nets`. A fragment has the same shape. Blocks are
// functions that return fragments. `merge` joins the fragments.
{
  local lamina = self,

  // Join fragments. Each part must have a unique refdes. Nets with the
  // same name join their pads. For other fields, the later fragment wins.
  merge(fragments)::
    std.foldl(function(acc, f) lamina.merge2(acc, f), fragments, {}),

  merge2(a, b)::
    local hasParts = std.objectHas(a, 'parts') || std.objectHas(b, 'parts');
    local hasNets = std.objectHas(a, 'nets') || std.objectHas(b, 'nets');
    local ap = std.get(a, 'parts', {});
    local bp = std.get(b, 'parts', {});
    local an = std.get(a, 'nets', {});
    local bn = std.get(b, 'nets', {});
    local dup = [r for r in std.objectFields(bp) if std.objectHas(ap, r)];
    assert std.length(dup) == 0 : 'more than one fragment has these parts: ' + std.join(', ', dup);
    local nets = an + {
      [n]: (
        if std.objectHas(an, n)
        then an[n] + bn[n] + { pads: std.set(an[n].pads + bn[n].pads) }
        else bn[n]
      )
      for n in std.objectFields(bn)
    };
    a + b
    + (if hasParts then { parts: ap + bp } else {})
    + (if hasNets then { nets: nets } else {})
    + (if std.objectHas(a, 'diff_pairs') || std.objectHas(b, 'diff_pairs') then { diff_pairs: std.get(a, 'diff_pairs', {}) + std.get(b, 'diff_pairs', {}) } else {}),

  // Put each part of a fragment in a group. A group is one instance of a
  // block. Groups of the same block can copy their layout to each other
  // (repeat_layout).
  group(name, fragment)::
    fragment + (
      if std.objectHas(fragment, 'parts') then {
        parts: { [r]: fragment.parts[r] + { group: name } for r in std.objectFields(fragment.parts) },
      } else {}
    ),

  // Make a net from pad references, for example 'R1.1'.
  net(pads, class=null)::
    { pads: pads } + (if class == null then {} else { class: class }),

  // Pad references of one part: pads('R1', [1, 2]) == ['R1.1', 'R1.2'].
  pads(refdes, numbers):: [refdes + '.' + std.toString(n) for n in numbers],

  // A rectangular outline with its lower-left corner at (x, y) and an
  // optional corner radius.
  rect(w, h, r=0, x=0, y=0)::
    if r <= 0 then {
      vertices: [
        { x: x, y: y },
        { x: x + w, y: y },
        { x: x + w, y: y + h },
        { x: x, y: y + h },
      ],
    } else {
      local arc(cx, cy) = { arc: { center: { x: cx, y: cy }, ccw: true } },
      vertices: [
        { x: x + r, y: y },
        { x: x + w - r, y: y } + arc(x + w - r, y + r),
        { x: x + w, y: y + r },
        { x: x + w, y: y + h - r } + arc(x + w - r, y + h - r),
        { x: x + w - r, y: y + h },
        { x: x + r, y: y + h } + arc(x + r, y + h - r),
        { x: x, y: y + h - r },
        { x: x, y: y + r } + arc(x + r, y + r),
      ],
    },

  // A round mechanical hole.
  hole(id, x, y, d):: { id: id, at: { x: x, y: y }, drill: { type: 'round', diameter: d } },

  // Four mounting holes, at the distance `inset` from the corners of a
  // w by h board.
  corner_holes(w, h, d, inset, x=0, y=0):: [
    lamina.hole('h1', x + inset, y + inset, d),
    lamina.hole('h2', x + w - inset, y + inset, d),
    lamina.hole('h3', x + inset, y + h - inset, d),
    lamina.hole('h4', x + w - inset, y + h - inset, d),
  ],
}
