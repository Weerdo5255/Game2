// Writes a small valid binary glTF model (a coloured box, 1 x 2 x 1, standing
// on the origin) for tests and as a stand-in while trying out model support.
//
//   node tools/make_glb.js out.glb [#rrggbb]
const fs = require('fs');
function boxGlb(hex) {
  const c = /^#[0-9a-f]{6}$/i.test(hex || '') ? [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16) / 255) : [0.8, 0.3, 0.3];
  const pos = [], nor = [], idx = [];
  const faces = [[[1, 0, 0], [0, 1, 0], [0, 0, 1]], [[-1, 0, 0], [0, 0, 1], [0, 1, 0]], [[0, 1, 0], [0, 0, 1], [1, 0, 0]], [[0, -1, 0], [1, 0, 0], [0, 0, 1]], [[0, 0, 1], [1, 0, 0], [0, 1, 0]], [[0, 0, -1], [0, 1, 0], [1, 0, 0]]];
  for (const [n, u, v] of faces) {
    const base = pos.length / 3;
    for (const [a, b] of [[-1, -1], [1, -1], [1, 1], [-1, 1]]) {
      const p = [0, 1, 2].map(k => n[k] * 0.5 + u[k] * a * 0.5 + v[k] * b * 0.5);
      pos.push(p[0], p[1] * 2 + 1, p[2]); nor.push(...n);   // 1 wide, 2 tall, feet at 0
    }
    idx.push(base, base + 1, base + 2, base, base + 2, base + 3);
  }
  const bin = Buffer.alloc(pos.length * 4 + nor.length * 4 + idx.length * 2);
  pos.forEach((v, i) => bin.writeFloatLE(v, i * 4));
  nor.forEach((v, i) => bin.writeFloatLE(v, pos.length * 4 + i * 4));
  idx.forEach((v, i) => bin.writeUInt16LE(v, (pos.length + nor.length) * 4 + i * 2));
  const json = {
    asset: { version: '2.0', generator: 'below-stairs make_glb' }, scene: 0, scenes: [{ nodes: [0] }], nodes: [{ mesh: 0, name: 'box' }],
    materials: [{ pbrMetallicRoughness: { baseColorFactor: [...c, 1], metallicFactor: 0, roughnessFactor: 0.8 } }],
    meshes: [{ primitives: [{ attributes: { POSITION: 0, NORMAL: 1 }, indices: 2, material: 0 }] }],
    buffers: [{ byteLength: bin.length }],
    bufferViews: [{ buffer: 0, byteOffset: 0, byteLength: pos.length * 4, target: 34962 }, { buffer: 0, byteOffset: pos.length * 4, byteLength: nor.length * 4, target: 34962 }, { buffer: 0, byteOffset: (pos.length + nor.length) * 4, byteLength: idx.length * 2, target: 34963 }],
    accessors: [{ bufferView: 0, componentType: 5126, count: pos.length / 3, type: 'VEC3', min: [-0.5, 0, -0.5], max: [0.5, 2, 0.5] }, { bufferView: 1, componentType: 5126, count: nor.length / 3, type: 'VEC3' }, { bufferView: 2, componentType: 5123, count: idx.length, type: 'SCALAR' }],
  };
  let text = Buffer.from(JSON.stringify(json)); while (text.length % 4) text = Buffer.concat([text, Buffer.from(' ')]);
  let body = bin; while (body.length % 4) body = Buffer.concat([body, Buffer.from([0])]);
  const head = Buffer.alloc(12); head.write('glTF', 0); head.writeUInt32LE(2, 4); head.writeUInt32LE(12 + 8 + text.length + 8 + body.length, 8);
  const chunk = (data, type) => { const h = Buffer.alloc(8); h.writeUInt32LE(data.length, 0); h.write(type, 4); return Buffer.concat([h, data]); };
  return Buffer.concat([head, chunk(text, 'JSON'), chunk(body, 'BIN\0')]);
}
module.exports = { boxGlb };
if (require.main === module) { fs.writeFileSync(process.argv[2] || 'box.glb', boxGlb(process.argv[3])); }
