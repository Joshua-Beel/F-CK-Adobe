"""Independent test-only fixture generated with ReportLab 4.4.9; no CI Python requirement."""
from pathlib import Path
from reportlab.pdfgen import canvas
from reportlab.lib.colors import Color, black, blue

path = Path(__file__).with_suffix('.pdf')
c = canvas.Canvas(str(path), pagesize=(612, 792), invariant=1)
c.drawString(60, 740, 'Independent standard single-select choice fields')
common = dict(fontName='Helvetica', fontSize=12, borderWidth=1, borderStyle='solid', borderColor=blue, textColor=black, fillColor=Color(.92, .96, 1), annotationFlags='print')
c.drawString(60, 704, 'Dropdown: display labels differ from export values')
c.acroForm.choice(name='Destination', x=60, y=664, width=220, height=28, options=[('North Hub', 'north-001'), ('South Hub', 'south-002'), ('Local Hub', 'local-003')], value='south-002', fieldFlags='combo', **common)
c.drawString(60, 624, 'List: all three source options are visible')
c.acroForm.listbox(name='Delivery', x=60, y=504, width=220, height=100, options=[('Print copy', 'print-001'), ('Email copy', 'email-002'), ('Local copy', 'local-003')], value='email-002', fieldFlags='', **common)
c.showPage()
c.save()
