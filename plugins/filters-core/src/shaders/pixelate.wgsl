fn hash(x: i32, y: i32, seed: u32) -> f32 {
    var n = bitcast<u32>(x) * 0x9E3779B1u + bitcast<u32>(y) * 0x85EBCA6Bu + seed * 0xC2B2AE35u;
    n ^= n >> 15u;
    n *= 0x2545F491u;
    n ^= n >> 13u;
    return f32(n & 65535u) / 65535.0;
}
fn value_noise(pos: vec2<f32>, seed: u32) -> f32 {
    let lo = floor(pos);
    let t = pos - lo;
    let s = t*t*(vec2<f32>(3.0)-2.0*t);
    let p = vec2<i32>(lo);
    let a = hash(p.x,p.y,seed); let b = hash(p.x+1,p.y,seed);
    let c = hash(p.x,p.y+1,seed); let d = hash(p.x+1,p.y+1,seed);
    let top = a+(b-a)*s.x;
    let bottom = c+(d-c)*s.x;
    return top+(bottom-top)*s.y;
}
fn effect(pos:vec2<i32>)->vec4<f32>{
    let mode=u32(args[0]);let original=read_pixel(pos);let cell=i32(args[1]);let p=vec2<f32>(pos);
    if mode==1u {
        let q=pos/cell;let lo=q*cell;let hi=min(lo+vec2(cell),vec2<i32>(i32(image.width),i32(image.height)));
        let jitter=vec2(value_noise(vec2<f32>(q),7717u),value_noise(vec2<f32>(q),7717u^0x5bf03635u));
        return straight(premul(read_pixel(lo+vec2<i32>(vec2<f32>(hi-lo)*jitter))));
    }
    if mode==2u {
        let q=pos/cell;let count=(vec2<i32>(i32(image.width),i32(image.height))+vec2(cell-1))/cell;var out=vec4(1.0);
        for(var cy=max(q.y-1,0);cy<=min(q.y+1,count.y-1);cy++){for(var cx=max(q.x-1,0);cx<=min(q.x+1,count.x-1);cx++){
            let grid=vec2<f32>(f32(cx),f32(cy));let center=grid*f32(cell)+f32(cell)*vec2(value_noise(grid,31u),value_noise(grid,61u));
            let radius=f32(cell)*0.45*(0.6+0.4*value_noise(grid,97u));
            if length(p+vec2(0.5)-center)<=radius {out=premul(read_pixel(vec2<i32>(center)));}
        }}return straight(out);
    }
    let kind=u32(args[2]);let size=args[1];var n=0.0;
    if kind<=3u {n=value_noise(p/size,4241u);if kind==2u {n=(n+value_noise(p*2.0,733u))/2.0;}}
    else {let runs=array<f32,3>(3.0,8.0,20.0);let run=size*runs[select(kind-7u,kind-4u,kind<=6u)];n=value_noise(vec2(p.x/(size*0.5),p.y/run),4241u);
        if kind>=7u {n=(n+value_noise(vec2(p.x/(size*2.0),p.y/(run*0.4)),8663u))/2.0;}}
    return vec4(select(vec3(0.0),vec3(1.0),original.rgb>vec3(n)),original.a);
}
