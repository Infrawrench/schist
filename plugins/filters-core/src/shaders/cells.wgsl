fn compute(i:u32){
    let cell=u32(args[0]);let cols=(shape.width+cell-1u)/cell;
    if args[1]==0.0 {
        let pixel=i/4u;let c=i%4u;let start=vec2(pixel%cols,pixel/cols)*cell;
        let end=min(start+vec2(cell),vec2(shape.width,shape.height));var sum=0.0;
        for(var y=start.y;y<end.y;y++){for(var x=start.x;x<end.x;x++){
            let j=(y*shape.width+x)*4u;sum+=src[j+c]*select(src[j+3u],1.0,c==3u);
        }}dst[i]=sum/f32((end.x-start.x)*(end.y-start.y));
    }else{
        let pixel=i/4u;let c=i%4u;let x=pixel%shape.width;let y=pixel/shape.width;let j=((y/cell)*cols+x/cell)*4u;let alpha=src[j+3u];
        dst[i]=alpha;if c!=3u {dst[i]=select(0.0,src[j+c]/max(alpha,0.000001),alpha>0.000001);}
    }
}
