"""Owned synthetic fixtures: tests geometry/detail, not a substitute for natural-image validation."""
import json,pathlib,random
from PIL import Image,ImageDraw,ImageFilter
root=pathlib.Path(__file__).resolve().parents[1]/"artifacts"/"evaluation"
root.mkdir(parents=True,exist_ok=True)
entries=[]
for index in range(24):
    rng=random.Random(600+index);w,h=1280,768;scale=3
    image=Image.new("RGB",(w,h),tuple(rng.randrange(100,190) for _ in range(3)))
    bg=ImageDraw.Draw(image)
    for _ in range(30):
        x=rng.randrange(w);y=rng.randrange(h);bg.rectangle((x,y,x+rng.randrange(10,80),y+rng.randrange(10,80)),fill=tuple(rng.randrange(70,210) for _ in range(3)))
    mask=Image.new("L",(w*scale,h*scale));draw=ImageDraw.Draw(mask)
    kind=["small","ring","thin","box","overlap","soft"][index%6]
    x=300+index*13%500;y=250+index*9%180;size=48 if kind=="small" else 180
    box=(x*scale,y*scale,(x+size)*scale,(y+size)*scale)
    if kind=="ring":
        draw.ellipse(box,fill=255);draw.ellipse(((x+40)*scale,(y+40)*scale,(x+size-40)*scale,(y+size-40)*scale),fill=0);point=(x+20,y+size//2)
    elif kind=="thin":
        draw.ellipse((x*scale,y*scale,(x+80)*scale,(y+100)*scale),fill=255)
        for j in range(12):draw.line(((x+35)*scale,(y+40)*scale,(x+size)*scale,(y+j*15)*scale),fill=255,width=3*scale)
        point=(x+35,y+40)
    else:
        if kind=="box":draw.rounded_rectangle(box,radius=8*scale,fill=255)
        else:draw.ellipse(box,fill=255)
        point=(x+size//2,y+size//2)
    alpha=mask.resize((w,h),Image.Resampling.LANCZOS)
    if kind=="soft":alpha=alpha.filter(ImageFilter.GaussianBlur(2))
    color=tuple(rng.randrange(10,75) for _ in range(3)) if index%2 else tuple(rng.randrange(220,255) for _ in range(3))
    foreground=Image.new("RGB",(w,h),color)
    if kind=="overlap":
        ImageDraw.Draw(image).ellipse((x-80,y-35,x+size-30,y+size-10),fill=(180,80,60))
    image=Image.composite(foreground,image,alpha)
    image.save(root/f"{index:02}.png");alpha.save(root/f"{index:02}-alpha.png")
    entries.append(dict(id=f"synthetic-{index:02}",image=f"{index:02}.png",alpha=f"{index:02}-alpha.png",point=point,category=kind,split="heldout" if index>=12 else "tuning"))
(root/"manifest.json").write_text(json.dumps(entries,indent=2))
print("Generated",len(entries),"owned, deterministically annotated fixtures")
