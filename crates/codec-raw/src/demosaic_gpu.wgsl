// src is the padded mosaic; codes follow the six scalar arguments.
fn code(x:i32,y:i32)->u32{return u32(args[6u+u32(y)*u32(args[1])+u32(x)]);}
fn at(x:i32,y:i32)->f32{return src[u32(y)*u32(args[1])+u32(x)];}
fn guide(x:i32,y:i32)->f32{return aux[u32(y)*u32(args[1])+u32(x)];}
fn difference(x:i32,y:i32)->f32{return at(x,y)-guide(x,y);}
fn wide(x:i32,y:i32,want:u32,fallback:f32)->f32{
    for(var r=3;r<=6;r++){
        var sum=0.0;var weight=0.0;
        for(var dy=-r;dy<=r;dy++){for(var dx=-r;dx<=r;dx++){
            if abs(dx)!=r && abs(dy)!=r {continue;}
            if code(x+dx,y+dy)==want {let w=1.0/f32(dx*dx+dy*dy);sum+=w*at(x+dx,y+dy);weight+=w;}
        }}
        if weight>0.0 {return sum/weight;}
    }return fallback;
}
fn compute(i:u32){
    let mode=u32(args[0]);let pw=i32(args[1]);let ph=i32(args[2]);
    if mode==1u || mode==3u || mode==4u {
        let x=i32(i%u32(pw));let y=i32(i/u32(pw));var pad=2;if mode==4u{pad=4;}
        if x<pad || y<pad || x+pad>=pw || y+pad>=ph {dst[i]=0.0;return;}
        let v=at(x,y);let own=code(x,y);
        if mode==1u {
            if own==1u {dst[i]=v;return;}
            let w=at(x-1,y);let e=at(x+1,y);let n=at(x,y-1);let s=at(x,y+1);
            let lh=2.0*v-at(x-2,y)-at(x+2,y);let lv=2.0*v-at(x,y-2)-at(x,y+2);
            let dh=abs(w-e)+abs(lh);let dv=abs(n-s)+abs(lv);
            let gh=0.5*(w+e)+0.25*lh;let gv=0.5*(n+s)+0.25*lv;
            var out=0.5*(gh+gv);if dh<dv{out=gh;}else if dv<dh{out=gv;}dst[i]=out;return;
        }
        if mode==4u && own==1u {dst[i]=v;return;}
        var sum_g=0.0;var weight_g=0.0;var sum_c=0.0;var weight_c=0.0;
        let here=guide(x,y);
        for(var dy=-2;dy<=2;dy++){for(var dx=-2;dx<=2;dx++){
            if dx==0 && dy==0 {continue;}let k=code(x+dx,y+dy);
            if k!=1u && (mode==3u || k!=own) {continue;}
            var w=1.0/f32(dx*dx+dy*dy);
            if mode==4u {w/=1.0+8.0*abs(guide(x+dx,y+dy)-here);}
            if k==1u{sum_g+=w*at(x+dx,y+dy);weight_g+=w;}else{sum_c+=w*at(x+dx,y+dy);weight_c+=w;}
        }}
        var out=v;if weight_g>0.0{out=sum_g/weight_g;if mode==4u && weight_c>0.0{out+=0.5*(v-sum_c/weight_c);}}
        dst[i]=out;return;
    }
    if i>=shape.width*shape.height {return;}
    let x=i32(i%shape.width)+6;let y=i32(i/shape.width)+6;
    let v=at(x,y);let own=code(x,y);var out=vec3(0.0);
    if mode==0u {
        let n=at(x,y-1);let s=at(x,y+1);let w=at(x-1,y);let e=at(x+1,y);
        if own==1u {
            let hor=(w+e)*0.5;let vert=(n+s)*0.5;
            if code(x-1,y)==0u {out=vec3(hor,v,vert);}else{out=vec3(vert,v,hor);}
        }else{
            let g=(n+s+w+e)*0.25;let d=(at(x-1,y-1)+at(x+1,y-1)+at(x-1,y+1)+at(x+1,y+1))*0.25;
            if own==0u{out=vec3(v,g,d);}else{out=vec3(d,g,v);}
        }
    }else if mode==2u {
        let g=guide(x,y);
        if own==1u {
            let hor=0.5*(difference(x-1,y)+difference(x+1,y));let vert=0.5*(difference(x,y-1)+difference(x,y+1));
            if code(x-1,y)==0u{out=vec3(g+hor,v,g+vert);}else{out=vec3(g+vert,v,g+hor);}
        }else{
            let nw=difference(x-1,y-1);let ne=difference(x+1,y-1);let sw=difference(x-1,y+1);let se=difference(x+1,y+1);
            let down=abs(nw-se);let up=abs(ne-sw);var other=0.25*(nw+ne+sw+se);
            if down<up{other=0.5*(nw+se);}else if up<down{other=0.5*(ne+sw);}other+=g;
            if own==0u{out=vec3(v,g,other);}else{out=vec3(other,g,v);}
        }
    }else{
        let directional=mode==5u;var sum=vec3(0.0);var weight=vec3(0.0);let g=guide(x,y);
        for(var dy=-2;dy<=2;dy++){for(var dx=-2;dx<=2;dx++){
            if dx==0 && dy==0{continue;}let k=code(x+dx,y+dy);if directional && k==1u{continue;}
            var w=1.0/f32(dx*dx+dy*dy);var sample=at(x+dx,y+dy);
            if directional {let qg=guide(x+dx,y+dy);w/=1.0+8.0*abs(qg-g);sample-=qg;}
            sum[k]+=w*sample;weight[k]+=w;
        }}
        for(var k=0u;k<3u;k++){
            if directional && k==1u {out[k]=g;}else if k==own{out[k]=v;}else if weight[k]>0.0{
                out[k]=sum[k]/weight[k];if directional{out[k]+=g;}
            }else{out[k]=wide(x,y,k,select(v,g,directional));}
        }
    }
    dst[i*3u]=out.r;dst[i*3u+1u]=out.g;dst[i*3u+2u]=out.b;
}
