// キャンバスと UI のテクスチャを画面に表示するシェーダー。
// キャンバスの透明部分は市松模様の上に premultiplied で合成する。UI は透明部分からキャンバスが見える。

struct View {
    // キャンバス px -> 画面 px: screen = mat2x2(linear) * canvas + pan
    linear: vec4<f32>,
    pan: vec2<f32>,
    screen_size: vec2<f32>,
    canvas_size: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var canvas_tex: texture_2d<f32>;
@group(0) @binding(2) var canvas_samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    // 2 三角形で矩形を描く
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let uv = corners[i];
    let c = uv * view.canvas_size;
    let screen = vec2(
        view.linear.x * c.x + view.linear.z * c.y,
        view.linear.y * c.x + view.linear.w * c.y,
    ) + view.pan;
    let ndc = vec2(screen.x / view.screen_size.x * 2.0 - 1.0, 1.0 - screen.y / view.screen_size.y * 2.0);
    var out: VsOut;
    out.pos = vec4(ndc, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(canvas_tex, canvas_samp, in.uv);
    let cell = floor(in.pos.xy / 8.0);
    let checker = select(0.8, 1.0, (i32(cell.x) + i32(cell.y)) % 2 == 0);
    let bg = vec3(checker);
    return vec4(c.rgb + bg * (1.0 - c.a), 1.0);
}

@fragment
fn fs_ui(in: VsOut) -> @location(0) vec4<f32> {
    // premultiplied のまま返し、ブレンドで下のキャンバスに重ねる
    return textureSample(canvas_tex, canvas_samp, in.uv);
}
