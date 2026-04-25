# Third-Party Licenses

This file contains the full license texts for upstream projects whose data
or copyrightable contributions are incorporated into the data files shipped
in this repository (`tags.csv`, `tags.dmap`).

The Rust and Python build/runtime dependencies listed in `CREDITS` are
linked dynamically and their license texts are not duplicated here; consult
the dependency's own repository for the full text.

---

## pydicom

`tags.csv` and `tags.dmap` include private DICOM tag entries (creator,
group, element-block-offset, VR, VM, name) that were derived from
[pydicom](https://github.com/pydicom/pydicom)'s `pydicom/_private_dict.py`.
Those entries are tagged with `sources=["pydicom"]` in our data files.

pydicom itself is released under the MIT license, with the private
dictionary portion further licensed under BSD-3-Clause (see GDCM below).

```
License file for pydicom, a pure-python DICOM library

Copyright (c) 2008-2020 Darcy Mason and pydicom contributors

Except for portions outlined below, pydicom is released under an MIT license:

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

---

## GDCM (Grassroots DICOM library)

pydicom's private tag dictionary (`pydicom/_private_dict.py`) is itself
generated from the GDCM project's private dictionary. Any entry in
`tags.csv` / `tags.dmap` tagged `sources=["pydicom"]` therefore traces
back to GDCM.

```
Program: GDCM (Grassroots DICOM). A DICOM library

Copyright (c) 2006-2016 Mathieu Malaterre
Copyright (c) 1993-2005 CREATIS
(CREATIS = Centre de Recherche et d'Applications en Traitement de l'Image)
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

 * Redistributions of source code must retain the above copyright notice,
   this list of conditions and the following disclaimer.

 * Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

 * Neither name of Mathieu Malaterre, or CREATIS, nor the names of any
   contributors (CNRS, INSERM, UCB, Universite Lyon I), may be used to
   endorse or promote products derived from this software without specific
   prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS ``AS IS''
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED. IN NO EVENT SHALL THE AUTHORS OR CONTRIBUTORS BE LIABLE FOR
ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```
