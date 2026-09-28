#!/usr/bin/env python3
"""Export the pretrained semantic guide used by background removal.

Torchvision DeepLabV3/MobileNetV3, COCO weights with VOC labels. No private
photographs are used to train this network. See subject-guide.json for hashes.
"""
import argparse,hashlib,json,io
from pathlib import Path
from model_archive import sidecar, write_archive
import torch
from torch import nn
from torch.nn import functional as F
import torchvision
from torchvision.models.segmentation import deeplabv3_mobilenet_v3_large,DeepLabV3_MobileNet_V3_Large_Weights

CLASSES=[3,8,10,12,13,15,17]  # bird, cat, cow, dog, horse, person, sheep
class Guide(nn.Module):
    def __init__(self,network):
        super().__init__();self.backbone=network.backbone;self.classifier=network.classifier
    def forward(self,x):
        logits=self.classifier(self.backbone(x)['out'])
        logits=F.interpolate(logits,size=(520,520),mode='bilinear',align_corners=False)
        return logits.softmax(1)[:,CLASSES].sum(1,keepdim=True)

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out',type=Path,default=Path('crates/neural/models/subject-guide.onnx.xz'))
    args=parser.parse_args();torch.set_num_threads(4)
    weights=DeepLabV3_MobileNet_V3_Large_Weights.COCO_WITH_VOC_LABELS_V1
    model=Guide(deeplabv3_mobilenet_v3_large(weights=weights,progress=False)).eval()
    args.out.parent.mkdir(parents=True,exist_ok=True)
    buffer=io.BytesIO()
    torch.onnx.export(model,torch.zeros(1,3,520,520),buffer,input_names=['rgb'],output_names=['subjects'],opset_version=17,dynamo=False)
    checkpoint=Path(torch.hub.get_dir())/'checkpoints'/Path(weights.url).name
    report={'model':'Torchvision DeepLabV3 MobileNetV3 Large semantic guide','weights_url':weights.url,'weights_sha256':hashlib.sha256(checkpoint.read_bytes()).hexdigest(),'torch':torch.__version__,'torchvision':torchvision.__version__,'input':[1,3,520,520],'preprocessing':'Antialiased triangle resize to 520x520; float sRGB, ImageNet mean/std.','categories':[weights.meta['categories'][i] for i in CLASSES],'output':'Sum of selected class softmax probabilities; not an alpha matte.','training':'Pretrained upstream on COCO with VOC labels; no Schist/private-photo fine-tuning.','source_license':'Torchvision BSD-3-Clause; see licenses/Torchvision-BSD-3-Clause.txt',**write_archive(args.out,buffer.getvalue())}
    sidecar(args.out,'.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
if __name__=='__main__':main()
