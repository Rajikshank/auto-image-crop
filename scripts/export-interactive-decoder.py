"""Pinned SAM 2.1 iterative point/box decoder with optional low-resolution mask prior."""
import pathlib,json,hashlib,sys
for stream in (sys.stdout,sys.stderr):
    if hasattr(stream,'reconfigure'):stream.reconfigure(encoding='utf-8',errors='backslashreplace')
import torch,numpy as np,onnxruntime as ort
from transformers import Sam2Model
from model_export_utils import canonicalize
root=pathlib.Path(__file__).resolve().parents[1];torch.set_num_threads(4);torch.manual_seed(0)
model=Sam2Model.from_pretrained(root/'models/reference-sam21',local_files_only=True).eval()
class Decoder(torch.nn.Module):
    def __init__(self,model):super().__init__();self.model=model
    def forward(self,points,labels,prior,has_prior,f0,f1,f2):
        sparse,dense=self.model.prompt_encoder(input_points=points,input_labels=labels,input_boxes=None,input_masks=prior)
        empty=self.model.prompt_encoder.no_mask_embed.weight.reshape(1,-1,1,1).expand(1,-1,64,64)
        dense=has_prior.reshape(1,1,1,1)*dense+(1-has_prior.reshape(1,1,1,1))*empty
        masks,scores,_,_=self.model.mask_decoder(image_embeddings=f2,image_positional_embeddings=self.model.get_image_wide_positional_embeddings(),sparse_prompt_embeddings=sparse,dense_prompt_embeddings=dense,multimask_output=True,high_resolution_features=[f0,f1])
        return scores,masks
decoder=Decoder(model).eval()
features=(torch.randn(1,32,256,256),torch.randn(1,64,128,128),torch.randn(1,256,64,64))
points=torch.tensor([[[[400.,400.],[100.,100.]]]]);labels=torch.tensor([[[1,0]]]);prior=torch.randn(1,1,256,256)
args=(points,labels,prior,torch.zeros(1),*features)
names=['input_points','input_labels','mask_input','has_mask_input','image_embeddings.0','image_embeddings.1','image_embeddings.2']
path=root/'models/sam21/interactive_decoder.onnx'
torch.onnx.export(decoder,args,str(path),input_names=names,output_names=['iou_scores','pred_masks'],dynamic_axes={'input_points':{2:'points'},'input_labels':{2:'points'}},opset_version=17,dynamo=False)
canonicalize(path);session=ort.InferenceSession(str(path),providers=['CPUExecutionProvider'])
report=[]
for with_prior in [False,True]:
    for with_box in [False,True]:
        p=points;lab=labels;box=torch.tensor([[[100.,120.,900.,880.]]]) if with_box else None
        if with_box:p=torch.cat([points,box.reshape(1,1,2,2)],dim=2);lab=torch.cat([labels,torch.tensor([[[2,3]]])],dim=2)
        with torch.no_grad():expected=model(image_embeddings=list(features),input_points=points,input_labels=labels,input_boxes=box,input_masks=prior if with_prior else None)
        values=(p,lab,prior,torch.tensor([float(with_prior)]),*features)
        actual=session.run(None,{k:v.numpy() for k,v in zip(names,values)})
        errors=[float(abs(a-b.numpy()).max()) for a,b in zip(actual,(expected.iou_scores,expected.pred_masks))]
        assert max(errors)<0.002,(with_prior,with_box,errors)
        report.append(dict(prior=with_prior,box=with_box,max_error=errors))
manifest=dict(file='sam21/interactive_decoder.onnx',bytes=path.stat().st_size,sha256=hashlib.sha256(path.read_bytes()).hexdigest(),upstream='Local verified iterative SAM 2.1 decoder; scripts/export-interactive-decoder.py')
(root/'models/interactive-decoder-manifest.json').write_text(json.dumps(manifest,indent=2))
(root/'artifacts/interactive-parity.json').write_text(json.dumps(report,indent=2));print(report)
