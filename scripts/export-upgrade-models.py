"""Local source exports. Requires explicitly installed reference checkpoints."""
import pathlib, json, hashlib, sys
# Torch's exporter emits Unicode progress markers even through Windows redirected pipes.
for stream in (sys.stdout,sys.stderr):
    if hasattr(stream,'reconfigure'):stream.reconfigure(encoding='utf-8',errors='backslashreplace')
import torch, numpy as np, onnxruntime as ort
from transformers import Sam2Model,RfDetrForInstanceSegmentation
from model_export_utils import canonicalize
root=pathlib.Path(__file__).resolve().parents[1]
torch.set_num_threads(4)
report={}
class Decoder(torch.nn.Module):
    def __init__(self,model):super().__init__();self.model=model
    def forward(self,points,labels,f0,f1,f2):
        out=self.model(image_embeddings=[f0,f1,f2],input_points=points,input_labels=labels,multimask_output=True)
        return out.iou_scores,out.pred_masks
model=Sam2Model.from_pretrained(root/'models/reference-sam21',local_files_only=True).eval()
decoder=Decoder(model).eval()
args=(torch.tensor([[[[400.,400.]]]]),torch.ones((1,1,1),dtype=torch.int64),torch.randn(1,32,256,256),torch.randn(1,64,128,128),torch.randn(1,256,64,64))
path=root/'models/sam21/point_decoder.onnx'
names=['input_points','input_labels','image_embeddings.0','image_embeddings.1','image_embeddings.2']
torch.onnx.export(decoder,args,str(path),input_names=names,output_names=['iou_scores','pred_masks'],dynamic_axes={'input_points':{2:'points'},'input_labels':{2:'points'}},opset_version=17,dynamo=False)
canonicalize(path)
with torch.no_grad(): expected=decoder(*args)
session=ort.InferenceSession(str(path),providers=['CPUExecutionProvider'])
actual=session.run(None,{k:v.numpy() for k,v in zip(names,args)})
report['sam21_point_decoder_mae']=[float(abs(a-b.numpy()).mean()) for a,b in zip(actual,expected)]
del model,decoder,session
class Detector(torch.nn.Module):
    def __init__(self,model):super().__init__();self.model=model
    def forward(self,image):
        out=self.model(pixel_values=image)
        return out.logits,out.pred_boxes,out.pred_masks
model=RfDetrForInstanceSegmentation.from_pretrained(root/'models/reference-rfdetr',local_files_only=True,attn_implementation='eager').eval()
detector=Detector(model).eval();args=(torch.randn(1,3,384,384),)
path=root/'models/rfdetr/model.onnx';path.parent.mkdir(exist_ok=True)
torch.onnx.export(detector,args,str(path),input_names=['pixel_values'],output_names=['logits','pred_boxes','pred_masks'],opset_version=18,dynamo=True)
canonicalize(path)
with torch.no_grad(): expected=detector(*args)
session=ort.InferenceSession(str(path),providers=['CPUExecutionProvider']);actual=session.run(None,{'pixel_values':args[0].numpy()})
report['rfdetr_mae']=[float(abs(a-b.numpy()).mean()) for a,b in zip(actual,expected)]
assert max(report['sam21_point_decoder_mae'])<0.0005,report
assert max(report['rfdetr_mae'])<0.005,report
manifest=json.loads((root/'models/upgrade-manifest.json').read_text())
for relative in ['sam21/point_decoder.onnx','rfdetr/model.onnx','rfdetr/model.onnx.data']:
    manifest=[e for e in manifest if e['file']!=relative]
    p=root/'models'/relative;manifest.append(dict(file=relative,bytes=p.stat().st_size,sha256=hashlib.sha256(p.read_bytes()).hexdigest(),upstream='Local source export; see scripts/export-upgrade-models.py'))
(root/'models/upgrade-manifest.json').write_text(json.dumps(manifest,indent=2))
(root/'artifacts/export-parity.json').write_text(json.dumps(report,indent=2));print(report)
