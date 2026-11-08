"""Remove non-executable exporter provenance before pinning portable graph hashes."""
import pathlib,onnx

def strip_debug_metadata(message):
    for field,value in list(message.ListFields()):
        if field.name in ('doc_string','metadata_props'):
            message.ClearField(field.name)
        elif field.message_type is not None:
            if field.is_repeated:
                for item in value:strip_debug_metadata(item)
            else:strip_debug_metadata(value)

def canonical_bytes(path):
    model=onnx.load(str(path),load_external_data=False)
    strip_debug_metadata(model)
    return model.SerializeToString(deterministic=True)

def canonicalize(path):
    path=pathlib.Path(path);data=canonical_bytes(path)
    temporary=path.with_suffix(path.suffix+'.canonical.tmp')
    temporary.write_bytes(data);temporary.replace(path)
