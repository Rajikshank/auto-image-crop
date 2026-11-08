# Third-party model and runtime notices

ObjectLift uses these separately licensed components. Model setup is explicit; normal application use stays offline. Preserve these notices when preparing a distribution. This document does not assign a license to ObjectLift itself.

| Component | Source / terms |
| --- | --- |
| MobileSAM | [Original](https://github.com/ChaoningZhang/MobileSAM), Apache-2.0. [Acly export](https://huggingface.co/Acly/MobileSAM), metadata MIT. Preserve both notices. |
| SAM 2.1 Small weights | [Meta checkpoint](https://huggingface.co/facebook/sam2.1-hiera-small), Apache-2.0. [SAM 2 license](https://github.com/facebookresearch/sam2/blob/main/LICENSE). |
| SAM 2.1 vision ONNX | [ONNX Community export](https://huggingface.co/onnx-community/sam2.1-hiera-small-ONNX), derived from the Meta checkpoint. Pinned in the manifest. |
| RF-DETR Seg Small weights | [Pinned checkpoint](https://huggingface.co/stevenbucaille/rf-detr-seg-small/tree/4e033cd357acd25ea21c74692dfef20ce75cb38c), model card Apache-2.0. [Original RF-DETR](https://github.com/roboflow/rf-detr). |
| ViTMatte weights | [HustVL checkpoint](https://huggingface.co/hustvl/vitmatte-small-composition-1k), model card Apache-2.0; [Xenova ONNX conversion](https://huggingface.co/Xenova/vitmatte-small-composition-1k). Upstream [ViTMatte code](https://github.com/hustvl/ViTMatte/blob/main/LICENSE) is MIT, copyright 2023 Hust Vision Lab. Checkpoint and code terms are distinct. |
| Transformers export code | [Hugging Face Transformers](https://github.com/huggingface/transformers), Apache-2.0. Development only. |
| ONNX Runtime | MIT; package license and third-party notices retained under models/runtime. |
| DirectML | Microsoft distribution terms retained under models/directml. |

Exact model revisions, byte sizes and SHA256 digests are in `scripts/upgrade-models.lock.json`. Source checkpoint files and Python training/export dependencies are not needed in the portable runtime.

The GitHub release supplies the application executable and Rust dependency notices. The setup script downloads model and runtime assets directly from their publishers with pinned checksums. Their terms apply separately from the MIT license for ObjectLift. License and notice files in the runtime archives remain in the installed model directory.
