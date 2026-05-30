---
title: Python quickstart
sidebar_label: Python quickstart
---

# Python quickstart

```sh
pip install dicom-map
```

```python
import dicom_map

# Use the wheel-bundled dictionary
d = dicom_map.open(dicom_map.bundled_dmap_path())
# ...or open one explicitly:
# d = dicom_map.open("tags.dmap")

print(len(d))   # ~19,521

# Public tag
t = d.lookup(0x0008, 0x0005)
print(t)        # {'vr': 'CS', 'keyword': 'SpecificCharacterSet', ...}

# Private tag (creator required)
t = d.lookup(0x0021, 0x0008, "Siemens: Thorax/Multix FD Lab Settings")
print(t["vr"], t["name"])
```

## With pydicom

```python
import pydicom
import dicom_map

d = dicom_map.open(dicom_map.bundled_dmap_path())

ds = pydicom.dcmread("scan.dcm")
for elem in ds:
    if elem.tag.is_private:
        creator = ds.private_creator(elem.tag.group, elem.tag.element >> 8)
        entry = d.lookup(elem.tag.group, elem.tag.element & 0xFF, creator)
        if entry:
            print(elem.tag, entry["name"], "=", elem.value)
```

The lookup expects the **block offset** for private tags (low byte
of element), matching the convention used in vendor conformance
statements.
