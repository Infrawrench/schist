fn compute(i:u32){
    if args[0]==0.0 {
        let cpp=u32(args[1]);let lw=u32(args[2]);let lh=u32(args[3]);var j=i%cpp;
        if cpp==1u {j=((i/shape.width+u32(args[4]))%lh)*lw+(i%shape.width)%lw;}
        let count=u32(args[5]);dst[i]=(src[i]-args[6u+j])*args[6u+count+j];return;
    }
    let p=i/3u;let c=i%3u;let x=p%shape.width;let y=p/shape.width;
    let w=u32(args[2]);let h=u32(args[3]);var sx=x;var sy=y;
    switch u32(args[6]) {
        case 1u:{sx=w-1u-x;}case 2u:{sx=w-1u-x;sy=h-1u-y;}case 3u:{sy=h-1u-y;}
        case 4u:{sx=y;sy=x;}case 5u:{sx=y;sy=h-1u-x;}case 6u:{sx=w-1u-y;sy=h-1u-x;}case 7u:{sx=w-1u-y;sy=x;}
        default:{}
    }
    let j=((sy+u32(args[5]))*u32(args[1])+sx+u32(args[4]))*3u;let m=7u+c*3u;
    dst[i]=max(args[m]*src[j]+args[m+1u]*src[j+1u]+args[m+2u]*src[j+2u],0.0);
}
