"""Compare pinned ViTMatte ONNX against its source checkpoint on identical inputs."""
import pathlib,json
import torch,numpy as np,onnxruntime as ort
from PIL import Image
from transformers import VitMatteForImageMatting,VitMatteImageProcessor
root=pathlib.Path(__file__).resolve().parents[1];torch.set_num_threads(4)
image=Image.open(root/'artifacts/evaluation/17.png').convert('RGB').crop((420,330,676,586))
trimap=Image.new('L',image.size,128)
processor=VitMatteImageProcessor.from_pretrained(root/'models/reference-vitmatte',local_files_only=True)
inputs=processor(images=image,trimaps=trimap,return_tensors='pt')
model=VitMatteForImageMatting.from_pretrained(root/'models/reference-vitmatte',local_files_only=True).eval()
with torch.no_grad(): expected=model(**inputs).alphas.numpy()
session=ort.InferenceSession(str(root/'models/vitmatte/model.onnx'),providers=['CPUExecutionProvider'])
actual=session.run(None,{'pixel_values':inputs.pixel_values.numpy()})[0]
report={'shape':list(actual.shape),'mae':float(abs(actual-expected).mean()),'max':float(abs(actual-expected).max())}
assert report['mae']<0.0001 and report['max']<0.005,report
(root/'artifacts/matte-parity.json').write_text(json.dumps(report,indent=2));print(report)
