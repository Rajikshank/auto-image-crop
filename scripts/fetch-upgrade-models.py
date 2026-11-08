"""Explicit model install. Downloads pinned data files; the application stays offline."""
import concurrent.futures, hashlib, json, pathlib, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCES = [
    ("onnx-community/sam2.1-hiera-small-ONNX", "a7df49d8de14b9d2e4504d1687b0d568f905fd8d", "sam21", ["onnx/vision_encoder.onnx", "onnx/vision_encoder.onnx_data", "onnx/prompt_encoder_mask_decoder.onnx", "onnx/prompt_encoder_mask_decoder.onnx_data"]),
    ("Xenova/vitmatte-small-composition-1k", "6bc1297f6140f055a227b6d2cfe8c093281f35d2", "vitmatte", ["onnx/model.onnx"]),
    ("facebook/sam2.1-hiera-small", "ee5bba1d82bb8749febdf90f45e84b687142ba03" , "reference-sam21", ["config.json","model.safetensors","preprocessor_config.json"]),
    ("hustvl/vitmatte-small-composition-1k", "6a58ad7646403c1df626fbd746900aec7361ea1d", "reference-vitmatte", ["config.json","model.safetensors","preprocessor_config.json"]),
    ("stevenbucaille/rf-detr-seg-small", "4e033cd357acd25ea21c74692dfef20ce75cb38c", "reference-rfdetr", ["config.json","model.safetensors","preprocessor_config.json"]),
]

def fetch(source):
    repo, revision, target, files = source
    metadata=json.load(urllib.request.urlopen(f"https://huggingface.co/api/models/{repo}/revision/{revision}?blobs=true"))
    entries={entry["rfilename"]:entry for entry in metadata["siblings"]}
    result=[]
    for name in files:
        path=ROOT/"models"/target/pathlib.PurePosixPath(name).name
        path.parent.mkdir(parents=True,exist_ok=True)
        url=f"https://huggingface.co/{repo}/resolve/{revision}/{name}"
        expected=entries[name].get("lfs",{}).get("sha256")
        if not path.exists():
            print(f"Downloading {repo}/{name}",flush=True)
            size=entries[name].get("lfs",{}).get("size",entries[name].get("size"))
            with open(str(path)+".partial","wb") as output:
                if size and size>4*1024*1024:
                    for start in range(0,size,4*1024*1024):
                        end=min(size-1,start+4*1024*1024-1)
                        request=urllib.request.Request(url+"?download=true",headers={"Range":f"bytes={start}-{end}"})
                        with urllib.request.urlopen(request,timeout=45) as response: data=response.read()
                        if len(data)!=end-start+1: raise RuntimeError(f"Invalid range response for {name}")
                        output.write(data)
                else:
                    with urllib.request.urlopen(url+"?download=true",timeout=45) as response: output.write(response.read())
            pathlib.Path(str(path)+".partial").replace(path)
        digest=hashlib.sha256(path.read_bytes()).hexdigest()
        if expected and digest!=expected: raise RuntimeError(f"Checksum mismatch: {path}")
        result.append(dict(file=str(path.relative_to(ROOT/"models")).replace("\\","/"),sha256=digest,bytes=path.stat().st_size,url=url,upstream=repo,revision=revision))
    return result

if __name__=="__main__":
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        manifest=[entry for group in pool.map(fetch,SOURCES) for entry in group]
    (ROOT/"models"/"upgrade-manifest.json").write_text(json.dumps(manifest,indent=2))
    print("Verified",len(manifest),"upgrade model files",flush=True)
