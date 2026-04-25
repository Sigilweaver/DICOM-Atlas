# dicom-map (Python)

Python bindings for the [`dicom-map`](..) Rust library: a memory-mapped
O(log n) dictionary of public and private DICOM tags.

## Install (from source)

```bash
pip install maturin
maturin develop --release -m dicom-map-py/pyproject.toml
```

## Usage

```python
import dicom_map

d = dicom_map.open("tags.dmap")
print(len(d))  # number of entries

# Public tag
d.lookup(0x0008, 0x0005)
# -> {'group': 8, 'element': 5, 'creator': '', 'keyword': 'SpecificCharacterSet',
#     'name': 'Specific Character Set', 'vr': 'CS', ...}

# Private tag (pass low byte of element)
d.lookup(0x0021, 0x0008, "Siemens: Thorax/Multix FD Lab Settings")
# -> {'vr': 'US', 'name': 'Auto Window Flag', 'block_offset': True, ...}

# Miss
d.lookup(0xFFFF, 0xFFFE) is None
```
