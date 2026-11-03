const $ = id => document.getElementById(id);
const state = {parts: [], colours: [], selected: null, colour: null, yaw: -.8, pitch: .6, zoom: 1, asset: null};
const meshes = new Map(), pending = new Map(), thumbnails = new Map();
let meshBytes = 0, preview, thumbnailRenderer, queue = [], pumping = false;
const number = value => Number(value).toLocaleString('en', {maximumFractionDigits: 3});
const hex = rgb => '#' + rgb.toString(16).padStart(6, '0');
const words = value => value.replaceAll('_', ' ');
const rgb = colour => [colour.rgb >> 16 & 255, colour.rgb >> 8 & 255, colour.rgb & 255].map(v => v / 255);
const make = (tag, text, className) => { const node = document.createElement(tag); if (text !== undefined) node.textContent = text; if (className) node.className = className; return node; };
const dot = (a, b) => a.reduce((sum, value, i) => sum + value * b[i], 0);
const unit = a => { const length = Math.hypot(...a); return a.map(v => v / length); };
const cross = (a, b) => [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]];

class SourceRenderer {
  constructor(canvas) {
    this.canvas = canvas;
    const gl = this.gl = canvas.getContext('webgl2', {alpha: false, antialias: true, preserveDrawingBuffer: true});
    if (!gl) throw Error('WebGL2 is unavailable. Piece data and source links remain available.');
    const compile = (type, source) => {
      const shader = gl.createShader(type); gl.shaderSource(shader, source); gl.compileShader(shader);
      if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw Error(gl.getShaderInfoLog(shader));
      return shader;
    };
    const vertex = compile(gl.VERTEX_SHADER, `#version 300 es
      layout(location=0) in vec3 position;
      layout(location=1) in vec3 normal;
      layout(location=2) in vec3 colour;
      uniform mat4 view, projection;
      uniform vec3 centre;
      out vec3 n, c;
      void main(){
        gl_Position=projection*view*vec4(position*vec3(1.,1.,1.2)-centre,1.);
        gl_PointSize=6.; n=normal/vec3(1.,1.,1.2); c=colour;
      }`);
    const fragment = compile(gl.FRAGMENT_SHADER, `#version 300 es
      precision highp float;
      in vec3 n, c;
      uniform vec3 base;
      uniform float alpha;
      uniform int mode;
      out vec4 outColour;
      void main(){
        if(mode==2 && length(gl_PointCoord-vec2(.5))>.5)discard;
        if(mode!=0){outColour=vec4(c,1.);return;}
        vec3 material=c.r<0.?base:c;
        vec3 normal=normalize(n);
        float light=.48+.38*max(dot(normal,normalize(vec3(-.6,-.7,1.))),0.)+.14*max(dot(normal,normalize(vec3(.7,.4,.5))),0.);
        outColour=vec4(material*light,alpha);
      }`);
    this.program = gl.createProgram(); gl.attachShader(this.program, vertex); gl.attachShader(this.program, fragment); gl.linkProgram(this.program);
    if (!gl.getProgramParameter(this.program, gl.LINK_STATUS)) throw Error(gl.getProgramInfoLog(this.program));
    gl.deleteShader(vertex); gl.deleteShader(fragment);
    this.uniforms = Object.fromEntries(['view', 'projection', 'centre', 'base', 'alpha', 'mode'].map(name => [name, gl.getUniformLocation(this.program, name)]));
    this.vao = gl.createVertexArray(); gl.bindVertexArray(this.vao);
    this.buffer = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, this.buffer);
    for (let i = 0; i < 3; i++) { gl.enableVertexAttribArray(i); gl.vertexAttribPointer(i, 3, gl.FLOAT, false, 36, i * 12); }
    canvas.addEventListener('webglcontextlost', event => { event.preventDefault(); $('preview-status').textContent = 'Graphics were reset. Reload the page to restore previews.'; });
  }
  draw(asset, colour, part, {yaw = -.8, pitch = .6, zoom = 1, interfaces = false} = {}) {
    const gl = this.gl, canvas = this.canvas;
    const bounds = asset.mesh.bounds;
    const centre = bounds.min.map((v, i) => (v + bounds.max[i]) / 2 * (i === 2 ? 1.2 : 1));
    const dimensions = bounds.min.map((v, i) => (bounds.max[i] - v) * (i === 2 ? 1.2 : 1));
    const radius = Math.max(.1, Math.hypot(...dimensions) / 2), distance = radius * 3;
    const eye = [Math.cos(yaw)*Math.cos(pitch), Math.sin(yaw)*Math.cos(pitch), Math.sin(pitch)].map(v => v * distance);
    const f = unit(eye.map(v => -v)), s = unit(cross(f, [0, 0, 1])), u = cross(s, f);
    const view = new Float32Array([s[0],u[0],-f[0],0,s[1],u[1],-f[1],0,s[2],u[2],-f[2],0,-dot(s,eye),-dot(u,eye),dot(f,eye),1]);
    const aspect = canvas.width / canvas.height;
    const height = radius * 1.13 / zoom * (aspect < 1 ? 1 / aspect : 1), width = height * aspect, near = .01, far = distance + radius * 5;
    const projection = new Float32Array([1/width,0,0,0,0,1/height,0,0,0,0,-2/(far-near),0,0,0,-(far+near)/(far-near),1]);
    gl.viewport(0, 0, canvas.width, canvas.height); gl.clearColor(.957, .973, .988, 1); gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.useProgram(this.program); gl.bindVertexArray(this.vao); gl.bindBuffer(gl.ARRAY_BUFFER, this.buffer);
    gl.uniformMatrix4fv(this.uniforms.view, false, view); gl.uniformMatrix4fv(this.uniforms.projection, false, projection);
    gl.uniform3fv(this.uniforms.centre, centre); gl.uniform3fv(this.uniforms.base, rgb(colour)); gl.uniform1f(this.uniforms.alpha, colour.alpha / 255); gl.uniform1i(this.uniforms.mode, 0);
    gl.enable(gl.DEPTH_TEST); gl.enable(gl.BLEND); gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    gl.bufferData(gl.ARRAY_BUFFER, asset.mesh.vertices, gl.STATIC_DRAW); gl.drawArrays(gl.TRIANGLES, 0, asset.mesh.vertices.length / 9);
    if (interfaces) {
      gl.disable(gl.DEPTH_TEST);
      const points = [], lines = [], vertex = (p, c) => [...p, 0, 0, 1, ...c];
      for (const grid of part.receiving_grids) for (let x = 0; x < grid.width; x++) for (let y = 0; y < grid.depth; y++) {
        points.push(...vertex([grid.first[0]+x, grid.first[1]+y, grid.first[2]], [.11, .31, .85]));
      }
      for (const port of part.connectors) {
        lines.push(...vertex(port.start, [.8, .42, .04]), ...vertex(port.end, [.8, .42, .04]));
        points.push(...vertex(port.start, [.8, .42, .04]), ...vertex(port.end, [.8, .42, .04]));
      }
      gl.uniform1i(this.uniforms.mode, 1); gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(lines), gl.STREAM_DRAW); gl.drawArrays(gl.LINES, 0, lines.length / 9);
      gl.uniform1i(this.uniforms.mode, 2); gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(points), gl.STREAM_DRAW); gl.drawArrays(gl.POINTS, 0, points.length / 9);
    }
    gl.flush();
  }
}

async function loadAsset(part) {
  if (meshes.has(part.id)) { const asset = meshes.get(part.id); meshes.delete(part.id); meshes.set(part.id, asset); return asset; }
  if (pending.has(part.id)) return pending.get(part.id);
  const request = (async () => {
    const response = await fetch('/api/parts/' + encodeURIComponent(part.id));
    if (!response.ok) throw Error(`Geometry request failed (${response.status}).`);
    const asset = await response.json();
    if (asset.mesh?.quality !== 'source' || !asset.mesh.vertices?.length) throw Error('Genuine source geometry is not available for this piece.');
    asset.mesh.vertices = new Float32Array(asset.mesh.vertices);
    while (meshes.size && (meshes.size >= 16 || meshBytes + asset.mesh.vertices.byteLength > 32 * 1024 * 1024)) {
      const oldest = meshes.keys().next().value; meshBytes -= meshes.get(oldest).mesh.vertices.byteLength; meshes.delete(oldest);
    }
    meshes.set(part.id, asset); meshBytes += asset.mesh.vertices.byteLength;
    return asset;
  })();
  pending.set(part.id, request);
  try { return await request; } finally { pending.delete(part.id); }
}

const observer = new IntersectionObserver(entries => {
  for (const entry of entries) if (entry.isIntersecting) {
    observer.unobserve(entry.target);
    queue.push({image: entry.target, part: state.parts.find(p => p.id === entry.target.dataset.part), colour: state.colour});
  }
  void pumpThumbnails();
}, {rootMargin: '180px'});

async function pumpThumbnails() {
  if (pumping || !thumbnailRenderer) return;
  pumping = true;
  try {
    while (queue.length) {
      const {image, part, colour} = queue.shift();
      if (!image.isConnected || colour.code !== state.colour.code) continue;
      const key = part.id + '/' + colour.code;
      try {
        let png = thumbnails.get(key);
        if (!png) {
          const asset = await loadAsset(part);
          if (!image.isConnected || colour.code !== state.colour.code) continue;
          thumbnailRenderer.draw(asset, colour, part);
          png = thumbnailRenderer.canvas.toDataURL('image/png');
          thumbnails.set(key, png);
          if (thumbnails.size > 256) thumbnails.delete(thumbnails.keys().next().value);
        }
        image.src = png; image.closest('button').dataset.geometry = 'source';
      } catch { image.closest('button').dataset.geometry = 'unavailable'; }
      await new Promise(requestAnimationFrame);
    }
  } finally { pumping = false; }
}

function family(part) {
  if (/^Tile\b/i.test(part.name)) return 'tiles';
  if (/^Plate\b/i.test(part.name)) return 'plates';
  if (/^Brick\b/i.test(part.name)) return part.connectors.length ? 'mechanical' : 'bricks';
  if (/\b(Plant|Tree|Flower)\b/i.test(part.name)) return 'scenery';
  if (/\bAnimal\b/i.test(part.name)) return 'animals';
  if (/\bMinifig\b/i.test(part.name)) return 'figures';
  if (/\b(Arch|Panel|Door|Window|Fence|Column|Slope)\b/i.test(part.name)) return 'architecture';
  return part.connectors.length ? 'mechanical' : 'specialty';
}

function feature(part, filter) {
  const kinds = part.connectors.map(port => port.kind).join(' ');
  return filter === 'all' || filter === 'studs' && part.top_studs > 0 || filter === 'receivers' && part.receiving_grids.length > 0 ||
    filter === 'grip' && /bar|grip/.test(kinds) || filter === 'axle' && /pin|axle|hole/.test(kinds) || filter === 'wheel' && /wheel|rim|tyre/.test(kinds);
}

function renderResults() {
  observer.disconnect(); queue = [];
  const terms = $('search').value.toLowerCase().trim().split(/\s+/).filter(Boolean), selectedFamily = $('family').value;
  const parts = state.parts.filter(part => {
    const text = [part.id, part.name, ...part.aliases, ...part.connectors.map(p => words(p.kind)), part.mass_source].join(' ').toLowerCase();
    return terms.every(term => text.includes(term)) && (selectedFamily === 'all' || family(part) === selectedFamily) && feature(part, $('feature').value);
  });
  $('part-count').textContent = `${parts.length} of ${state.parts.length} pieces`;
  $('empty').hidden = parts.length !== 0;
  const cards = parts.map(part => {
    const li = make('li'), card = make('button', undefined, 'part-card'); card.type = 'button'; card.dataset.part = part.id;
    card.setAttribute('aria-pressed', String(state.selected?.id === part.id)); card.setAttribute('aria-label', `${part.name}, ${part.id}`);
    const holder = make('span', undefined, 'card-image'), image = make('img'); image.alt = `${part.name}, genuine source geometry`; image.dataset.part = part.id; image.width = 256; image.height = 192;
    holder.append(image); card.append(holder, make('code', part.id), make('span', part.name, 'piece-name'));
    const label = part.connectors.length ? [...new Set(part.connectors.map(p => words(p.kind)))].slice(0, 2).join(', ') : part.top_studs ? `${part.top_studs} top studs` : `${part.receiving_grids.length} receiving ${part.receiving_grids.length === 1 ? 'grid' : 'grids'}`;
    card.append(make('span', label, 'piece-features')); card.addEventListener('click', () => { void selectPart(part, true); }); li.append(card); return li;
  });
  $('part-results').replaceChildren(...cards);
  for (const image of $('part-results').querySelectorAll('img')) observer.observe(image);
}

function fact(name, value) { $('part-facts').append(make('dt', name), make('dd', value)); }
function sourceLink(text, url) { const link = make('a', text); if (url.startsWith('https://')) { link.href = url; link.target = '_blank'; link.rel = 'noopener noreferrer'; } return link; }
function updateURL() {
  const url = new URL(location.href); if (state.selected) url.searchParams.set('part', state.selected.id); if (state.colour) url.searchParams.set('colour', state.colour.code); history.replaceState(null, '', url);
}
function drawPreview() {
  if (!preview || !state.asset || !state.colour) return;
  const canvas = $('part-preview'), ratio = Math.min(2, devicePixelRatio || 1);
  canvas.width = Math.max(1, Math.round(canvas.clientWidth * ratio)); canvas.height = Math.max(1, Math.round(canvas.clientHeight * ratio));
  preview.draw(state.asset, state.colour, state.selected, {yaw: state.yaw, pitch: state.pitch, zoom: state.zoom, interfaces: $('show-interfaces').checked});
}
function setInspector(open) {
  $('inspector-body').hidden = !open;
  $('detail-toggle').setAttribute('aria-expanded', String(open));
  $('detail-toggle').textContent = open ? 'Hide details' : 'Show details';
  if (open) requestAnimationFrame(drawPreview);
}
async function selectPart(part, focus = false) {
  state.selected = part; state.asset = null; state.yaw = -.8; state.pitch = .6; state.zoom = 1;
  $('part-id').textContent = part.id; $('part-name').textContent = part.name; $('preview-status').textContent = 'Loading genuine source geometry…'; $('retry-preview').hidden = true;
  $('part-facts').replaceChildren(); fact('Catalogue mass', `${number(part.mass_kg * 1000)} g`); fact('Nominal body', part.nominal_dimensions_mm.map(number).join(' × ') + ' mm'); fact('Top studs', part.top_studs); fact('Receiving grids', part.receiving_grids.length);
  if (part.set_count !== undefined) fact('Recorded set usage', number(part.set_count));
  if (part.aliases.length) fact('Official aliases', part.aliases.join(', '));
  const interfaces = make('ul');
  for (const grid of part.receiving_grids) interfaces.append(make('li', `Receiving grid ${grid.width} × ${grid.depth}, first centre [${grid.first.map(number).join(', ')}]`));
  for (const port of part.connectors) interfaces.append(make('li', `${words(port.kind)}: ${number(port.radius_m * 1000)} mm radius`));
  $('interface-details').replaceChildren(make('strong', 'Registered interfaces'), interfaces);
  if (!interfaces.children.length) interfaces.append(make('li', 'No additional interface datums are registered.'));
  const credits = $('part-credits'); credits.replaceChildren();
  const massURLs = [...new Set(part.mass_source.match(/https:\/\/www\.bricklink\.com\/v2\/catalog\/catalogitem\.page\?P=[A-Za-z0-9]+/g) || [])];
  credits.append(make('p', massURLs.length > 1 ? 'Mass record: sourced component sum.' : 'Mass record: catalogue average.'));
  for (const url of massURLs) { credits.append(sourceLink(`BrickLink ${new URL(url).searchParams.get('P')}`, url), document.createTextNode(' ')); }
  credits.append(make('p', part.mass_source), make('p', `Library revision ${part.library_revision}`));
  document.querySelectorAll('.part-card').forEach(card => card.setAttribute('aria-pressed', String(card.dataset.part === part.id)));
  updateURL();
  if (focus && matchMedia('(max-width:760px)').matches) { setInspector(true); $('part-name').focus({preventScroll: true}); document.querySelector('.inspector').scrollIntoView({behavior: matchMedia('(prefers-reduced-motion:reduce)').matches ? 'instant' : 'smooth'}); }
  try {
    const asset = await loadAsset(part); if (state.selected.id !== part.id) return;
    state.asset = asset; drawPreview();
    const envelope = asset.mesh.bounds.min.map((v, i) => (asset.mesh.bounds.max[i] - v) * (i === 2 ? 9.6 : 8));
    fact('Rendered envelope', envelope.map(number).join(' × ') + ' mm');
    $('preview-status').textContent = preview ? `Source mesh, ${number(asset.mesh.vertices.length / 27)} triangles. Blue dots show receiving centres; amber lines show mechanical interfaces.` : 'Source geometry loaded. WebGL2 is required for its preview.';
    const list = make('ul'); for (const credit of asset.credits) { const item = make('li'); item.append(sourceLink(credit.file, credit.source_url), document.createTextNode(` — ${credit.author}; ${credit.license}`)); list.append(item); } credits.append(list);
  } catch (error) { if (state.selected.id === part.id) { $('preview-status').textContent = error.message; $('retry-preview').hidden = false; } }
}

function renderColours() {
  const colours = state.colours.filter(colour => $('all-colours').checked || colour.recommended || colour.code === state.colour.code);
  $('selected-colour').textContent = `${state.colour.name} (${state.colour.code}) ${hex(state.colour.rgb)}`;
  $('colour-swatches').replaceChildren(...colours.map(colour => {
    const button = make('button', undefined, 'swatch'); button.type = 'button'; button.title = `${colour.name} (${colour.code})`; button.setAttribute('aria-label', button.title); button.setAttribute('aria-pressed', String(colour.code === state.colour.code));
    button.style.setProperty('--swatch', hex(colour.rgb)); button.style.setProperty('--alpha', colour.alpha / 255);
    const channels = rgb(colour); button.style.setProperty('--check', dot(channels, [.2126, .7152, .0722]) > .5 ? '#183047' : '#ffffff');
    button.append(make('span', colour.code === state.colour.code ? '✓' : ''));
    button.addEventListener('click', () => { state.colour = colour; renderColours(); renderResults(); drawPreview(); updateURL(); }); return button;
  }));
}

function installControls() {
  const mobile = matchMedia('(max-width:760px)');
  setInspector(!mobile.matches);
  mobile.addEventListener('change', () => setInspector(!mobile.matches));
  $('detail-toggle').addEventListener('click', () => setInspector($('inspector-body').hidden));
  document.querySelector('.back-to-results').addEventListener('click', event => {
    if (!mobile.matches) return;
    event.preventDefault(); setInspector(false);
    const card = [...document.querySelectorAll('.part-card')].find(node => node.dataset.part === state.selected?.id);
    card?.focus();
  });
  $('filters').addEventListener('submit', event => event.preventDefault());
  for (const id of ['search', 'family', 'feature']) $(id).addEventListener('input', renderResults);
  $('clear-filters').addEventListener('click', () => { $('filters').reset(); renderResults(); $('search').focus(); });
  $('all-colours').addEventListener('change', renderColours); $('show-interfaces').addEventListener('change', drawPreview);
  $('reset-view').addEventListener('click', () => { state.yaw = -.8; state.pitch = .6; state.zoom = 1; drawPreview(); });
  $('retry-preview').addEventListener('click', () => { void selectPart(state.selected); });
  const canvas = $('part-preview'); let pointer = null;
  canvas.addEventListener('pointerdown', event => { pointer = [event.clientX, event.clientY]; canvas.setPointerCapture(event.pointerId); });
  canvas.addEventListener('pointermove', event => { if (!pointer) return; state.yaw -= (event.clientX-pointer[0])*.01; state.pitch = Math.max(-1.3, Math.min(1.3, state.pitch+(event.clientY-pointer[1])*.008)); pointer = [event.clientX,event.clientY]; drawPreview(); });
  for (const event of ['pointerup', 'pointercancel', 'lostpointercapture']) canvas.addEventListener(event, () => { pointer = null; });
  canvas.addEventListener('wheel', event => { event.preventDefault(); state.zoom = Math.max(.35, Math.min(4, state.zoom * Math.exp(-event.deltaY*.001))); drawPreview(); }, {passive: false});
  canvas.addEventListener('keydown', event => { if (!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key)) return; event.preventDefault(); if (event.key === 'ArrowLeft') state.yaw += .15; if (event.key === 'ArrowRight') state.yaw -= .15; if (event.key === 'ArrowUp') state.pitch = Math.min(1.3,state.pitch+.12); if (event.key === 'ArrowDown') state.pitch = Math.max(-1.3,state.pitch-.12); drawPreview(); });
  new ResizeObserver(drawPreview).observe(canvas);
}

async function start() {
  installControls();
  try {
    preview = new SourceRenderer($('part-preview'));
    const canvas = document.createElement('canvas'); canvas.width = 256; canvas.height = 192; thumbnailRenderer = new SourceRenderer(canvas);
  } catch (error) { $('preview-status').textContent = error.message; }
  try {
    const response = await fetch('/api/catalog'); if (!response.ok) throw Error(`Catalogue request failed (${response.status}). Reload to try again.`);
    const catalogue = await response.json(); state.parts = catalogue.parts; state.colours = catalogue.colors;
    const params = new URLSearchParams(location.search);
    state.colour = state.colours.find(c => c.code === Number(params.get('colour') ?? 71)) || state.colours.find(c => c.code === 71) || state.colours[0];
    const selected = state.parts.find(p => p.id === params.get('part') || p.aliases.includes(params.get('part'))) || state.parts.find(p => p.id === '3001.dat') || state.parts[0];
    renderColours(); renderResults(); await selectPart(selected);
    document.body.dataset.catalogueReady = 'true';
  } catch (error) { $('catalogue-error').hidden = false; $('catalogue-error').textContent = error.message; $('part-count').textContent = 'Catalogue unavailable'; }
}
void start();
