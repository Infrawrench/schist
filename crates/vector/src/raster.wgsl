// One pixel per invocation; exact horizontal spans, the reference vertical samples.
fn crossing(e:u32, y:f32)->vec2<f32>{
    let b=e*5u;let x0=src[b];let y0=src[b+1u];let x1=src[b+2u];let y1=src[b+3u];
    if y<min(y0,y1) || y>=max(y0,y1) {return vec2(0.0,0.0);}
    return vec2(x0+(y-y0)/(y1-y0)*(x1-x0),src[b+4u]);
}
fn inside(w:i32)->bool {if args[2]==0.0{return (w & 1)!=0;}return w!=0;}
fn compute(i:u32){
    let left=f32(i%shape.width)+args[0];let right=left+1.0;
    let top=args[1]+f32(i/shape.width);let edges=arrayLength(&src)/5u;
    var coverage=0.0;
    let subsamples=u32(args[3]);
    for(var s=0u;s<subsamples;s++){
        let y=top+(f32(s)+0.5)/f32(subsamples);var cursor=left;
        // Only crossings inside this pixel split its coverage interval.
        for(var iteration=0u;iteration<=edges;iteration++){
            var next=right;var winding=0;
            for(var e=0u;e<edges;e++){
                let p=crossing(e,y);if p.y==0.0{continue;}
                if p.x<=cursor {winding+=select(i32(p.y),1,args[2]==0.0);}
                else if p.x<next{next=p.x;}
            }
            if inside(winding){coverage+=(next-cursor)/f32(subsamples);}
            if next>=right{break;}cursor=next;
        }
    }
    dst[i]=floor(clamp(coverage,0.0,1.0)*255.0+0.5);
}
