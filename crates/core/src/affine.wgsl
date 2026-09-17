// args: inverse matrix, source origin/size, destination origin, filter, footprint.
// Axis terms are prepared once per row/column, preserving CPU boundary rounding.
fn point(x:u32,y:u32,sx:u32,sy:u32)->vec2<f32> {
    let xi=15u+x*10u+sx*2u;let yi=15u+shape.width*10u+y*10u+sy*2u;
    return vec2((args[xi]+args[yi])+args[4],(args[xi+1u]+args[yi+1u])+args[5]);
}
fn nearest(x: f32) -> i32 { return i32(sign(x)*floor(abs(x)+0.5)); }
fn fetch(x: i32, y: i32, c: u32) -> f32 {
    let w = i32(args[8]); let h = i32(args[9]);
    let xx = x-i32(args[6]); let yy = y-i32(args[7]);
    if shape.channels == 1u && (xx<0 || yy<0 || xx>=w || yy>=h) { return 0.0; }
    let j = u32(clamp(yy,0,h-1)*w+clamp(xx,0,w-1))*shape.channels;
    if shape.channels == 1u || c == shape.channels-1u { return src[j+c]; }
    return src[j+c]*src[j+shape.channels-1u];
}
fn weights(t: f32) -> vec4<f32> {
    let t2=t*t; let t3=t2*t;
    return 0.5*vec4(-t3+2.0*t2-t,3.0*t3-5.0*t2+2.0,-3.0*t3+4.0*t2+t,t3-t2);
}
fn sample_at(p: vec2<f32>, c: u32) -> f32 {
    if shape.channels != 1u && (args[13]>2.0 || args[14]>2.0) {
        let half = max(vec2(args[13],args[14])*0.5,vec2(0.5));
        let lo=vec2(nearest(p.x-half.x),nearest(p.y-half.y));
        let hi=max(vec2(nearest(p.x+half.x),nearest(p.y+half.y)),lo+vec2(1));
        var sum=0.0; var n=0.0;
        for(var y=lo.y;y<hi.y;y++){ for(var x=lo.x;x<hi.x;x++){ sum+=fetch(x,y,c); n+=1.0; } }
        return sum/n;
    }
    if args[12] == 0.0 { return fetch(nearest(p.x),nearest(p.y),c); }
    let base=vec2<i32>(floor(p)); let f=p-floor(p);
    var sum=0.0;
    if args[12] == 2.0 {
        let wx=weights(f.x); let wy=weights(f.y);
        for(var y=0u;y<4u;y++){ for(var x=0u;x<4u;x++){
            let weight=wx[x]*wy[y]; if weight!=0.0 { sum+=fetch(base.x+i32(x)-1,base.y+i32(y)-1,c)*weight; }
        } }
    } else {
        for(var y=0;y<2;y++){ for(var x=0;x<2;x++){
            let weight=select(1.0-f.x,f.x,x==1)*select(1.0-f.y,f.y,y==1);
            if weight!=0.0 { sum+=fetch(base.x+x,base.y+y,c)*weight; }
        } }
    }
    return sum;
}
fn compute(i: u32) {
    if i>=shape.width*shape.height { return; }
    let x=i%shape.width;let y=i/shape.width;
    let p=point(x,y,0u,0u)-vec2(0.5);
    if shape.channels==1u { dst[i]=clamp(floor(sample_at(p,0u)+0.5),0.0,255.0); return; }
    var hits=0u;
    for(var sy=0u;sy<4u;sy++){ for(var sx=0u;sx<4u;sx++){
        let q=point(x,y,sx+1u,sy+1u);
        if q.x>=args[6] && q.y>=args[7] && q.x<args[6]+args[8] && q.y<args[7]+args[9] { hits++; }
    } }
    let alpha=shape.channels-1u;
    var a=sample_at(p,alpha);
    let cubic=args[12]==2.0 && args[13]<=2.0 && args[14]<=2.0;
    if cubic { a=clamp(a,0.0,1.0); }
    let is_nearest=args[12]==0.0 && args[13]<=2.0 && args[14]<=2.0;
    let empty=select(a<=0.000001,a<=0.0,is_nearest);
    for(var c=0u;c<alpha;c++){
        var v=sample_at(p,c);
        if cubic { v=clamp(v,0.0,a); }
        if empty || hits==0u { v=0.0; } else { v/=a; }
        dst[i*shape.channels+c]=v;
    }
    dst[i*shape.channels+alpha]=select(select(clamp(a,0.0,1.0),a,is_nearest)*f32(hits)/16.0,0.0,empty);
}
