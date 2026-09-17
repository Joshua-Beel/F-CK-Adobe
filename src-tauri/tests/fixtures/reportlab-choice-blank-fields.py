"""Independent pypdf blank variation of the populated ReportLab standard fields."""
from pathlib import Path
import re
from pypdf import PdfReader, PdfWriter
from pypdf.generic import NameObject, TextStringObject, DecodedStreamObject

root = Path(__file__).parent
writer = PdfWriter(clone_from=PdfReader(root / 'reportlab-choice-fields.pdf'))
for reference in writer._root_object['/AcroForm']['/Fields']:
    field = reference.get_object()
    normal = field['/AP']['/N']
    data = normal.get_data().decode('ascii')
    if field['/Ff'] & 131072:
        data, count = re.subn(r'BT\n/Helv 12 Tf\n0 0 0 rg\n1 0 0 1 4 14 Tm\n\(South Hub\) Tj\nET\n', '', data)
        assert count == 1
    else:
        data = data.replace('0.600006 0.756866 0.854904 rg\n2 66 216 16 re\nf\n', '')
        data = data.replace('BT\n0 g\n4 70 Td\n(Email copy) Tj', 'BT\n0 0 0 rg\n4 70 Td\n(Email copy) Tj')
    stream = DecodedStreamObject()
    for key, value in normal.items():
        if key not in ['/Filter', '/Length']:
            stream[NameObject(key)] = value
    stream.set_data(data.encode('ascii'))
    field['/AP'][NameObject('/N')] = writer._add_object(stream)
    field[NameObject('/V')] = TextStringObject('')
    field.pop('/I', None)
writer.write(root / 'reportlab-choice-blank-fields.pdf')
