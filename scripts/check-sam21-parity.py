"""Offline upstream/ONNX tensor parity. Diagnostic only; no model promotion."""
import pathlib,json,time
import numpy as np
import torch,onnxruntime as ort
from PIL import Image
from transformers import Sam2Model,Sam2Processor
root=pathlib.Path(__file__).resolve().parents[1]
torch.set_num_threads(4)
model=Sam2Model.from_pretrained(root/'models/reference-sam21',local_files_only=True).eval()
processor=Sam2Processor.from_pretrained(root/'models/reference-sam21',local_files_only=True)
image=Image.open(root/'artifacts/truck.jpg').convert('RGB')
inputs=processor(images=image,input_points=[[[[750,500]]]],input_boxes=[[[0,0,image.width,image.height]]],return_tensors='pt')
with torch.no_grad():
    ref=model(**{k:v for k,v in inputs.items() if k not in ('original_sizes','reshaped_input_sizes')})
enc=ort.InferenceSession(str(root/'models/sam21/vision_encoder.onnx'),providers=['CPUExecutionProvider'])
features=enc.run(None,{'pixel_values':inputs.pixel_values.numpy()})
report={'features':[]}
for a,b in zip(features,ref.image_embeddings):
    b=b.numpy();report['features'].append({'shape':list(a.shape),'mae':float(abs(a-b).mean()),'max':float(abs(a-b).max())})
print(json.dumps(report,indent=2),flush=True)
dec=ort.InferenceSession(str(root/'models/sam21/prompt_encoder_mask_decoder.onnx'),providers=['CPUExecutionProvider'])
values={'input_points':inputs.input_points.numpy(),'input_labels':np.ones(inputs.input_points.shape[:-1],dtype=np.int64),'input_boxes':inputs.input_boxes.numpy()}
values.update({f'image_embeddings.{i}':v for i,v in enumerate(features)})
out=dec.run(None,values)
for meta,a in zip(dec.get_outputs(),out):
    b=getattr(ref,meta.name,None)
    report[meta.name]={'shape':list(a.shape),'mae':float(abs(a-b.numpy()).mean()) if b is not None and a.shape==tuple(b.shape) else None}
mask=processor.post_process_masks(ref.pred_masks,inputs.original_sizes)[0][0]
scores=ref.iou_scores.numpy().flatten();best=int(scores.argmax())
Image.fromarray(mask[best].numpy().astype('uint8')*255).save(root/'artifacts/sam21-reference-mask.png')
(root/'artifacts/sam21-parity.json').write_text(json.dumps(report,indent=2));print(json.dumps(report,indent=2))

