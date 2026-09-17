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
fn fbm(p:vec2<f32>,seed:u32,n:u32)->f32{var sum=0.0;var amp=0.5;var freq=1.0;var norm=0.0;
    for(var o=0u;o<n;o++){sum+=value_noise(p*freq,seed+o*7919u)*amp;norm+=amp;amp*=0.5;freq*=2.0;}return sum/max(norm,0.000001);}
fn trunc_fract(p:vec2<f32>)->vec2<f32>{return p-trunc(p);}
fn surface(kind:u32,p:vec2<f32>,scale:f32,seed:u32)->f32{
    let uv=p/max(scale,1.0);let tau=6.283185307179586;
    if kind==0u{let weave=(sin(uv.x*tau)+sin(uv.y*tau))*0.25+0.5;return weave*0.35+fbm(uv*2.0,seed,3u)*0.45+value_noise(p,seed)*0.2;}
    if kind==1u{return fbm(uv,seed,3u)*0.6+value_noise(p,seed^0x5bd1u)*0.4;}
    if kind==2u{let warp=clamp(abs(sin(uv.x*tau))*0.6+abs(sin(uv.y*tau*0.5))*0.4,0.0,1.0);return warp*0.65+value_noise(p*vec2(1.3,0.9),seed)*0.35;}
    let shift=select(0.5,0.0,i32(floor(uv.y))%2==0);let c=trunc_fract(uv+vec2(shift,0.0));let mortar=select(0.85,0.25,c.x<0.06||c.y<0.12);return mortar*0.8+value_noise(p*0.5,seed)*0.2;
}
fn glass_height(p:vec2<f32>)->f32{
    let kind=u32(args[3]);if kind==0u{return fbm(p/max(args[2]*2.0,1.0),337u,3u);}
    if kind==1u{let uv=trunc_fract(p/args[4]);return (abs(uv.x-0.5)+abs(uv.y-0.5))*0.7;}
    return surface(kind-2u,p,args[4],149u);
}
fn texture_height(p:vec2<f32>)->f32{let value=surface(u32(args[1]),p,args[2],61u);return select(value,1.0-value,args[6]!=0.0);}
fn effect(pos:vec2<i32>)->vec4<f32>{
    let mode=u32(args[0]);let p=vec2<f32>(pos);
    if mode==0u{let xy=p+vec2(0.5);let gx=glass_height(xy+vec2(1.0,0.0))-glass_height(xy-vec2(1.0,0.0));let gy=glass_height(xy+vec2(0.0,1.0))-glass_height(xy-vec2(0.0,1.0));return straight(sample_premul(xy+vec2(gx,gy)*args[1]*1.2-vec2(0.5)));}
    if mode==1u{let xy=p+vec2(0.5);let a=fbm(xy/args[1],1049u,2u)-0.5;let b=fbm(xy/args[1]+vec2(31.0,17.0),1049u,2u)-0.5;return straight(sample_premul(xy+vec2(a,b)*args[2]*2.0-vec2(0.5)));}
    let gx=texture_height(p+vec2(1.0,0.0))-texture_height(p-vec2(1.0,0.0));let gy=texture_height(p+vec2(0.0,1.0))-texture_height(p-vec2(0.0,1.0));let shade=1.0+(gx*args[4]+gy*args[5])*args[3]*6.0;let original=read_pixel(pos);return vec4(clamp(original.rgb*shade,vec3(0.0),vec3(1.0)),original.a);
}
