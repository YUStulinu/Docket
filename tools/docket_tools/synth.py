"""Synthetic Romanian personal documents, photographed, with their truth.

Ten kinds of documents a person keeps: RCA and CASCO policies, the vehicle registration (talon)
with its ITP stamp, an ITP report, a passport, an identity card, a driving licence, warranty
certificates, a two-page rent contract and utility bills. Each is drawn as a clean page, then
photographed (photo.py): on a table, at an angle, unevenly lit, blurred and compressed. A few are
flat scans.

The truth of each document (its kind, the values that matter, its deadlines, the exact text of each
page, the page's corners in the photo) and questions about it, with their answers and the page
that holds them, go into truth.json. The people, companies' customers, numbers and plates are
invented; the institutions and insurers are real names, as they appear on such documents.

    cd tools && python -m docket_tools.synth --out ../testdata/synthetic
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import random
import textwrap
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

from .photo import photograph

ROOT = Path(__file__).resolve().parents[2]
FONTS = ROOT / "native" / "third_party" / "fonts"
TODAY = dt.date(2026, 10, 27)

FIRST = ["Ion", "Andrei", "Mihai", "Alexandru", "Elena", "Maria", "Ioana", "Andreea", "Cristina", "Radu", "Bogdan", "Ana", "Ștefan", "Mădălina", "Gabriel", "Irina"]
LAST = ["Popescu", "Ionescu", "Popa", "Dumitrescu", "Stan", "Stoica", "Gheorghe", "Matei", "Ciobanu", "Constantinescu", "Marinescu", "Țurcanu", "Răducanu", "Lungu"]
STREETS = ["Str. Lalelelor", "Bd. Unirii", "Str. Mihai Eminescu", "Calea Victoriei", "Str. Zorilor", "Bd. Iuliu Maniu", "Str. Fabricii", "Aleea Teiului"]
CITIES = [("București", "B"), ("Cluj-Napoca", "CJ"), ("Iași", "IS"), ("Timișoara", "TM"), ("Brașov", "BV"), ("Constanța", "CT")]
CARS = [("DACIA", "LOGAN", 1461), ("DACIA", "DUSTER", 1332), ("VOLKSWAGEN", "GOLF", 1598), ("SKODA", "OCTAVIA", 1968), ("RENAULT", "CLIO", 999), ("TOYOTA", "COROLLA", 1798)]
INSURERS = ["Allianz-Țiriac Asigurări S.A.", "Groupama Asigurări S.A.", "Omniasig Vienna Insurance Group S.A.", "Generali România Asigurare Reasigurare S.A.", "Asirom Vienna Insurance Group S.A."]
SHOPS = ["eMAG", "Altex România", "Flanco Retail", "Dedeman", "Media Galaxy"]
PRODUCTS = [
    ("Mașină de spălat rufe", "Samsung", "WW90T554DAW"),
    ("Frigider", "Arctic", "AK60406E40NFW"),
    ("Televizor LED", "LG", "55UR78003LK"),
    ("Laptop", "Lenovo", "IdeaPad Slim 5"),
    ("Aspirator robot", "Roborock", "S8"),
    ("Cuptor cu microunde", "Whirlpool", "MWP 254 W"),
    ("Telefon mobil", "Samsung", "Galaxy A55"),
]
SUPPLIERS = [("Enel Energie Muntenia S.A.", "energie electrică", "ENM"), ("Engie Romania S.A.", "gaze naturale", "EGR"), ("Digi Romania S.A.", "internet și televiziune", "DGR"), ("Apa Nova București S.A.", "apă și canalizare", "APN")]
MONTHS_RO = ["ianuarie", "februarie", "martie", "aprilie", "mai", "iunie", "iulie", "august", "septembrie", "octombrie", "noiembrie", "decembrie"]
MONTHS_EN = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"]

LEGAL = [
    "Prezenta poliță a fost emisă în conformitate cu prevederile Legii nr. 132/2017 privind asigurarea obligatorie de răspundere civilă auto pentru prejudicii produse terților prin accidente de vehicule și tramvaie și ale normelor emise de Autoritatea de Supraveghere Financiară.",
    "Asigurătorul acordă despăgubiri pentru prejudiciile de care asiguratul răspunde în baza legii față de terțe persoane prejudiciate prin accidente de vehicule, precum și pentru cheltuielile făcute de asigurat în procesul civil.",
    "În caz de producere a unui eveniment asigurat, contactați asigurătorul în termen de 24 de ore și completați formularul de constatare amiabilă a accidentului.",
]
WARRANTY_TERMS = [
    "Garanția se acordă conform Legii nr. 449/2003 și OUG nr. 140/2021 privind anumite aspecte referitoare la contractele de vânzare de bunuri.",
    "Termenul de garanție curge de la data livrării produsului către consumator. Produsul va fi reparat sau înlocuit gratuit în cazul unui defect de fabricație.",
    "Garanția nu acoperă defectele cauzate de transport, montaj sau utilizare necorespunzătoare, de fluctuațiile de tensiune sau de intervenția persoanelor neautorizate.",
]


def ro_date(d: dt.date) -> str:
    return d.strftime("%d.%m.%Y")


def add_months(d: dt.date, n: int) -> dt.date:
    y, m = divmod(d.month - 1 + n, 12)
    y += d.year
    m += 1
    for day in (d.day, 30, 29, 28):
        try:
            return dt.date(y, m, day)
        except ValueError:
            continue
    raise ValueError(d)


class Sheet:
    """A page with a writing cursor; remembers every line it draws (the page's true text)."""

    def __init__(self, w: int, h: int, margin: int, scale: float = 1.0, paper=(255, 255, 255)):
        self.img = Image.new("RGB", (w, h), paper)
        self.draw = ImageDraw.Draw(self.img)
        self.w, self.h, self.margin, self.scale = w, h, margin, scale
        self.y = margin
        self.lines: list[str] = []
        self._fonts: dict = {}

    def font(self, size: int, bold: bool = False, family: str = "sans"):
        name = {"sans": "DejaVuSans", "serif": "DejaVuSerif", "mono": "DejaVuSansMono"}[family] + ("-Bold" if bold else "")
        key = (name, size)
        if key not in self._fonts:
            self._fonts[key] = ImageFont.truetype(str(FONTS / f"{name}.ttf"), int(size * self.scale))
        return self._fonts[key]

    def text(self, s: str, size=30, bold=False, family="sans", align="left", color=(25, 25, 30), gap=0.45, x=None):
        f = self.font(size, bold, family)
        tw = self.draw.textlength(s, font=f)
        if x is None:
            x = {"left": self.margin, "center": (self.w - tw) / 2, "right": self.w - self.margin - tw}[align]
        self.draw.text((x, self.y), s, font=f, fill=color)
        self.lines.append(s)
        self.y += int(size * self.scale * (1 + gap))

    def pair(self, label: str, value: str, size=30, family="sans"):
        """`label value` with the label in bold."""
        fb, f = self.font(size, True, family), self.font(size, False, family)
        self.draw.text((self.margin, self.y), label, font=fb, fill=(25, 25, 30))
        lw = self.draw.textlength(label + " ", font=fb)
        self.draw.text((self.margin + lw, self.y), value, font=f, fill=(25, 25, 30))
        self.lines.append(f"{label} {value}")
        self.y += int(size * self.scale * 1.45)

    def para(self, s: str, size=26, family="serif", color=(40, 40, 45)):
        f = self.font(size, False, family)
        avg = self.draw.textlength("abcdefghijklmnopqrstuvwxyz", font=f) / 26
        for line in textwrap.wrap(s, width=int((self.w - 2 * self.margin) / avg)):
            self.text(line, size=size, family=family, color=color, gap=0.35)
        self.y += int(size * self.scale * 0.4)

    def rule(self, color=(120, 120, 130)):
        self.draw.line([(self.margin, self.y), (self.w - self.margin, self.y)], fill=color, width=max(1, int(2 * self.scale)))
        self.y += int(16 * self.scale)

    def band(self, color, height=60):
        """A colored header band (drawn, not text)."""
        self.draw.rectangle([0, 0, self.w, int(height * self.scale)], fill=color)

    def stamp(self, x, y, r, color=(40, 70, 160)):
        """A round stamp (ink, not text): noise for the OCR to ignore."""
        r = int(r * self.scale)
        for k in range(3):
            self.draw.ellipse([x - r + k * 9, y - r + k * 9, x + r - k * 9, y + r - k * 9], outline=color, width=4)

    def space(self, px):
        self.y += int(px * self.scale)


@dataclass
class Doc:
    id: str
    kind: str
    pages: list[Sheet]
    fields: dict
    deadlines: list[dict]
    questions: list[dict] = field(default_factory=list)
    flat: bool = False


class World:
    def __init__(self, seed: int):
        self.r = random.Random(seed)
        self.people = []
        # Distinct names: "the passport of Irina" must mean one person.
        firsts, lasts = self.r.sample(FIRST, 4), self.r.sample(LAST, 4)
        for first, last in zip(firsts, lasts):
            city, county = self.r.choice(CITIES)
            self.people.append({"first": first, "last": last, "city": city, "county": county,
                                "address": f"{self.r.choice(STREETS)} nr. {self.r.randint(1, 120)}, {city}",
                                "birth": dt.date(self.r.randint(1965, 2000), self.r.randint(1, 12), self.r.randint(1, 28))})
        self.cars = []
        for p in self.people[:3]:
            make, model, cc = self.r.choice(CARS)
            plate = f"{p['county']}-{self.r.randint(10, 99) if p['county'] != 'B' else self.r.randint(100, 999)}-{''.join(self.r.choice('ABCDEFGHJKLMNPRSTUVXYZ') for _ in range(3))}"
            vin = "".join(self.r.choice("ABCDEFGHJKLMNPRSTUVWXYZ0123456789") for _ in range(17))
            vin = "UU1" + vin[3:] if make == "DACIA" else "WVW" + vin[3:] if make == "VOLKSWAGEN" else "TMB" + vin[3:] if make == "SKODA" else vin
            self.cars.append({"owner": p, "make": make, "model": model, "cc": cc, "plate": plate, "vin": vin,
                              "first_reg": dt.date(self.r.randint(2012, 2022), self.r.randint(1, 12), self.r.randint(1, 28))})

    def day_between(self, a: dt.date, b: dt.date) -> dt.date:
        return a + dt.timedelta(days=self.r.randint(0, (b - a).days))

    def num(self, n: int) -> str:
        return "".join(str(self.r.randint(0, 9)) for _ in range(n))


A4 = (1654, 2339)  # 200 DPI
CARD = (1012, 638)  # ID-1 at 300 DPI
PASSPORT = (1476, 1039)  # 125 x 88 mm at 300 DPI


def rca(w: World, i: int, car: dict, before: dt.date | None = None) -> Doc:
    """An RCA policy; with `before`, the previous (expired) policy, ending the day before it."""
    p = car["owner"]
    start = w.day_between(TODAY - dt.timedelta(days=300), TODAY + dt.timedelta(days=30))
    months = w.r.choice([6, 12, 12])
    if before is not None:
        months = 12
        start = add_months(before, -12)
    end = add_months(start, months) - dt.timedelta(days=1)
    insurer = w.r.choice(INSURERS)
    s = Sheet(*A4, margin=110)
    s.band((0, 82, 155), 70)
    s.space(40)
    s.text("ASIGURARE OBLIGATORIE DE RĂSPUNDERE CIVILĂ AUTO", 34, True, align="center")
    s.text("POLIȚĂ DE ASIGURARE RCA", 44, True, align="center")
    series = f"RO/{w.r.randint(10, 40)}/X{w.r.randint(10, 99)}/{w.r.choice(['AA', 'BB', 'XZ', 'KL'])}"
    number = w.num(10)
    s.text(f"Seria {series} Nr. {number}", 30, align="center")
    s.space(30)
    s.rule()
    s.pair("Asigurător:", insurer)
    s.pair("Asigurat / Contractant:", f"{p['last'].upper()} {p['first'].upper()}")
    s.pair("Adresa:", p["address"])
    s.pair("Vehicul:", f"{car['make']} {car['model']}, categoria M1, {car['cc']} cm3")
    s.pair("Număr de înmatriculare:", car["plate"].replace("-", " "))
    s.pair("Serie șasiu (VIN):", car["vin"])
    s.rule()
    s.pair("Perioada de valabilitate:", f"de la {ro_date(start)} până la {ro_date(end)}")
    s.pair("Prima de asigurare:", f"{w.r.randint(600, 2400)},00 lei")
    s.pair("Data emiterii:", ro_date(start - dt.timedelta(days=w.r.randint(0, 5))))
    s.space(30)
    for t in w.r.sample(LEGAL, 3):
        s.para(t)
    s.stamp(1250, s.y + 150, 120)
    fields = {"plate": car["plate"], "vin": car["vin"], "insurer": insurer}
    deadlines = [{"kind": "rca_expiry", "date": end.isoformat()}]
    q = [{"question": f"Până când e valabil RCA-ul pentru {car['plate']}?", "page": 1, "date": end.isoformat()},
         {"question": f"La ce firmă am asigurarea RCA pentru {car['make'].title()} {car['model'].title()}?", "page": 1, "text": insurer.split()[0]}]
    return Doc(f"rca-{i}", "rca", [s], fields, deadlines, q)


def casco(w: World, i: int, car: dict) -> Doc:
    p = car["owner"]
    start = w.day_between(TODAY - dt.timedelta(days=330), TODAY - dt.timedelta(days=10))
    end = add_months(start, 12) - dt.timedelta(days=1)
    insurer = w.r.choice(INSURERS)
    s = Sheet(*A4, margin=110)
    s.band((150, 20, 40), 50)
    s.space(40)
    s.text(insurer, 32, True)
    s.text("POLIȚĂ DE ASIGURARE CASCO", 46, True, align="center")
    s.text(f"Nr. poliță: CA{w.num(9)}", 30, align="center")
    s.space(30)
    s.pair("Asigurat:", f"{p['first']} {p['last']}")
    s.pair("Autoturism:", f"{car['make']} {car['model']}, nr. înmatriculare {car['plate']}")
    s.pair("Sumă asigurată:", f"{w.r.randint(8, 30) * 1000} EUR")
    s.pair("Franșiză:", f"{w.r.choice([0, 100, 200, 300])} EUR")
    s.pair("Valabilă de la", f"{ro_date(start)} până la {ro_date(end)}")
    s.pair("Rate:", "4 rate trimestriale")
    s.space(20)
    s.para("Polița acoperă avariile și furtul autovehiculului asigurat, pe teritoriul României și al țărilor din Spațiul Economic European, în condițiile generale anexate.")
    s.para("Daunele se avizează în termen de 48 de ore de la producerea evenimentului, la numărul de telefon de pe verso.")
    fields = {"plate": car["plate"], "insurer": insurer}
    deadlines = [{"kind": "casco_expiry", "date": end.isoformat()}]
    q = [{"question": f"Când expiră asigurarea CASCO pentru {car['plate']}?", "page": 1, "date": end.isoformat()}]
    return Doc(f"casco-{i}", "casco", [s], fields, deadlines, q)


def talon(w: World, i: int, car: dict) -> Doc:
    p = car["owner"]
    itp = w.day_between(TODAY - dt.timedelta(days=700), TODAY - dt.timedelta(days=30))
    itp_end = add_months(itp, 24) - dt.timedelta(days=1)
    car["itp"], car["itp_end"] = itp, itp_end
    s = Sheet(*A4, margin=120, paper=(236, 244, 236))
    s.band((40, 110, 60), 40)
    s.space(50)
    s.text("ROMÂNIA", 36, True, align="center")
    s.text("CERTIFICAT DE ÎNMATRICULARE", 44, True, align="center")
    s.text(f"Seria {w.r.choice(['C', 'H', 'R'])}{w.num(9)}", 28, align="center")
    s.space(30)
    s.pair("A. Număr de înmatriculare:", car["plate"].replace("-", " "))
    s.pair("B. Data primei înmatriculări:", ro_date(car["first_reg"]))
    s.pair("C.1.1. Nume:", p["last"].upper())
    s.pair("C.1.2. Prenume:", p["first"].upper())
    s.pair("C.1.3. Adresa:", p["address"])
    s.pair("D.1. Marca:", car["make"])
    s.pair("D.3. Model:", car["model"])
    s.pair("E. Număr de identificare:", car["vin"])
    s.pair("J. Categoria vehiculului:", "M1")
    s.pair("P.1. Capacitate cilindrică:", f"{car['cc']} cm3")
    s.pair("S.1. Număr de locuri:", "5")
    s.space(40)
    s.rule()
    s.text("INSPECȚIE TEHNICĂ PERIODICĂ (ITP)", 32, True)
    s.pair("Data ITP:", ro_date(itp))
    s.pair("Valabil până la:", ro_date(itp_end))
    s.pair("Stația:", f"RAR {p['city']}, cod {w.r.randint(100, 999)}")
    s.stamp(1300, s.y - 60, 110, (120, 40, 40))
    fields = {"plate": car["plate"], "vin": car["vin"], "vehicle": f"{car['make']} {car['model']}"}
    deadlines = [{"kind": "itp_expiry", "date": itp_end.isoformat()}]
    q = [{"question": f"Când trebuie să fac ITP la mașina {car['plate']}?", "page": 1, "date": itp_end.isoformat()},
         {"question": f"Care e seria de șasiu a mașinii {car['plate']}?", "page": 1, "text": car["vin"]}]
    return Doc(f"talon-{i}", "vehicle_registration", [s], fields, deadlines, q)


def itp_report(w: World, i: int, car: dict) -> Doc:
    """The report of the inspection that the registration's ITP stamp records: the same dates."""
    when, nxt = car["itp"], car["itp_end"]
    s = Sheet(*A4, margin=110)
    s.text("REGISTRUL AUTO ROMÂN", 30, True)
    s.text(f"Stația ITP nr. {w.r.randint(1000, 9999)} - {car['owner']['city']}", 28)
    s.space(30)
    s.text("RAPORT DE INSPECȚIE TEHNICĂ PERIODICĂ", 42, True, align="center")
    s.text(f"Nr. {w.num(7)} din {ro_date(when)}", 30, align="center")
    s.space(30)
    s.pair("Număr de înmatriculare:", car["plate"])
    s.pair("Marca / Model:", f"{car['make']} {car['model']}")
    s.pair("VIN:", car["vin"])
    s.pair("Kilometraj:", f"{w.r.randint(40, 250) * 1000} km")
    s.pair("Data inspecției:", ro_date(when))
    s.pair("Rezultat:", "ADMIS")
    s.pair("Următoarea inspecție până la:", ro_date(nxt))
    s.space(20)
    s.para("Defecte constatate: niciun defect major. Recomandare: verificarea uzurii plăcuțelor de frână la următoarea revizie.", family="sans")
    s.para("Inspectorul tehnic certifică faptul că vehiculul a fost inspectat conform Ordinului MTI nr. 2133/2005.", family="sans")
    fields = {"plate": car["plate"], "vin": car["vin"]}
    deadlines = [{"kind": "itp_expiry", "date": nxt.isoformat()}]
    q = [{"question": f"Ce rezultat a avut inspecția tehnică din {ro_date(when)}?", "page": 1, "text": "ADMIS"}]
    return Doc(f"itp-{i}", "itp", [s], fields, deadlines, q)


def mrz_name(p):
    return f"{p['last'].upper()}<<{p['first'].upper()}".replace("Ș", "S").replace("Ț", "T").replace("Ă", "A").replace("Î", "I").replace("Â", "A")


def passport(w: World, i: int, p: dict) -> Doc:
    issued = w.day_between(TODAY - dt.timedelta(days=3400), TODAY - dt.timedelta(days=400))
    expiry = add_months(issued, w.r.choice([60, 120]))
    number = w.num(9)
    s = Sheet(*PASSPORT, margin=60, paper=(238, 232, 245))
    s.space(10)
    s.text("ROMÂNIA  ROMANIA  ROUMANIE", 26, True, align="center")
    s.text("PAȘAPORT  PASSPORT  PASSEPORT", 30, True, align="center")
    x0 = 470
    s.draw.rectangle([60, s.y + 10, 420, s.y + 470], fill=(200, 195, 210))  # the photo
    rows = [
        ("Tip/Type", "P", "Cod/Code", "ROU", "Nr. pașaport/Passport No.", number),
    ]
    s.text(f"Tip/Type P   Cod/Code ROU   Nr./No. {number}", 24, x=x0)
    s.text("Nume/Surname", 18, color=(90, 80, 110), x=x0, gap=0.2)
    s.text(p["last"].upper(), 28, True, x=x0)
    s.text("Prenume/Given names", 18, color=(90, 80, 110), x=x0, gap=0.2)
    s.text(p["first"].upper(), 28, True, x=x0)
    s.text("Cetățenie/Nationality ROMÂNĂ/ROU", 22, x=x0)
    b = p["birth"]
    s.text(f"Data nașterii/Date of birth {b.day:02d} {MONTHS_EN[b.month - 1]} {b.year}", 22, x=x0)
    s.text(f"Locul nașterii/Place of birth {p['city'].upper()}", 22, x=x0)
    s.text(f"Data eliberării/Date of issue {issued.day:02d} {MONTHS_EN[issued.month - 1]} {issued.year}", 22, x=x0)
    s.text(f"Data expirării/Date of expiry {expiry.day:02d} {MONTHS_EN[expiry.month - 1]} {expiry.year}", 22, x=x0)
    s.text("Autoritatea/Authority D.G.P.", 22, x=x0)
    s.y = PASSPORT[1] - 160
    line1 = f"P<ROU{mrz_name(p)}".ljust(44, "<")[:44]
    line2 = f"{number}0ROU{b.strftime('%y%m%d')}1M{expiry.strftime('%y%m%d')}2<<<<<<<<<<<<<<04"[:44]
    s.text(line1, 30, family="mono")
    s.text(line2, 30, family="mono")
    _ = rows
    fields = {"document_number": number, "holder": f"{p['first']} {p['last']}"}
    deadlines = [{"kind": "passport_expiry", "date": expiry.isoformat()}]
    q = [{"question": f"Când expiră pașaportul lui {p['first']}?", "page": 1, "date": expiry.isoformat()}]
    return Doc(f"passport-{i}", "passport", [s], fields, deadlines, q)


def id_card(w: World, i: int, p: dict) -> Doc:
    issued = w.day_between(TODAY - dt.timedelta(days=3000), TODAY - dt.timedelta(days=200))
    expiry = add_months(issued, 120)
    series = f"{p['county'] if len(p['county']) == 2 else 'RX'}"
    number = w.num(6)
    s = Sheet(*CARD, margin=36, paper=(230, 238, 248))
    s.text("ROMÂNIA  ROUMANIE  ROMANIA", 22, True, align="center", gap=0.2)
    s.text("CARTE DE IDENTITATE  IDENTITY CARD", 24, True, align="center")
    s.draw.rectangle([36, s.y, 290, s.y + 300], fill=(190, 200, 215))
    x0 = 320
    s.text(f"SERIA {series} NR {number}", 22, True, x=x0)
    s.text("Nume/Nom/Last name", 15, color=(80, 90, 110), x=x0, gap=0.1)
    s.text(p["last"].upper(), 24, True, x=x0, gap=0.25)
    s.text("Prenume/Prénom/First name", 15, color=(80, 90, 110), x=x0, gap=0.1)
    s.text(p["first"].upper(), 24, True, x=x0, gap=0.25)
    s.text(f"Domiciliu: {p['address']}", 16, x=x0, gap=0.3)
    s.text(f"Emisă de SPCLEP {p['city']}", 16, x=x0, gap=0.3)
    s.text(f"Valabilitate {issued.strftime('%d.%m.%y')}-{ro_date(expiry)}", 20, True, x=x0)
    s.y = CARD[1] - 100
    s.text(f"IDROU{mrz_name(p)}".ljust(36, "<")[:36], 24, family="mono", gap=0.2)
    s.text(f"{series}{number}<{w.r.randint(0, 9)}ROU{p['birth'].strftime('%y%m%d')}{w.r.randint(0, 9)}M{expiry.strftime('%y%m%d')}"[:36].ljust(36, "<"), 24, family="mono", gap=0.2)
    fields = {"document_number": f"{series} {number}", "holder": f"{p['first']} {p['last']}"}
    deadlines = [{"kind": "id_card_expiry", "date": expiry.isoformat()}]
    q = [{"question": f"Până când e valabil buletinul lui {p['first']} {p['last']}?", "page": 1, "date": expiry.isoformat()}]
    return Doc(f"id-{i}", "id_card", [s], fields, deadlines, q)


def licence(w: World, i: int, p: dict) -> Doc:
    issued = w.day_between(TODAY - dt.timedelta(days=3400), TODAY - dt.timedelta(days=300))
    expiry = add_months(issued, 120)
    number = w.num(9)
    s = Sheet(*CARD, margin=36, paper=(248, 232, 236))
    s.text("PERMIS DE CONDUCERE   ROMÂNIA", 24, True, gap=0.3)
    s.draw.rectangle([36, s.y + 10, 290, s.y + 310], fill=(215, 195, 200))
    x0 = 320
    s.text(f"1. {p['last'].upper()}", 24, True, x=x0, gap=0.3)
    s.text(f"2. {p['first'].upper()}", 24, True, x=x0, gap=0.3)
    s.text(f"3. {ro_date(p['birth'])} {p['city'].upper()}", 20, x=x0, gap=0.3)
    s.text(f"4a. {ro_date(issued)}   4c. DRPCIV {p['city'].upper()}", 20, x=x0, gap=0.3)
    s.text(f"4b. {ro_date(expiry)}", 20, True, x=x0, gap=0.3)
    s.text(f"5. {number}", 20, x=x0, gap=0.3)
    s.text("9. AM/B1/B", 20, x=x0, gap=0.3)
    fields = {"document_number": number, "holder": f"{p['first']} {p['last']}"}
    deadlines = [{"kind": "licence_expiry", "date": expiry.isoformat()}]
    q = [{"question": f"Când expiră permisul de conducere al lui {p['first']}?", "page": 1, "date": expiry.isoformat()}]
    return Doc(f"licence-{i}", "driving_licence", [s], fields, deadlines, q)


def warranty(w: World, i: int, p: dict) -> Doc:
    product, brand, model = PRODUCTS[i % len(PRODUCTS)]
    sold = w.day_between(TODAY - dt.timedelta(days=900), TODAY - dt.timedelta(days=20))
    months = w.r.choice([24, 24, 36, 60])
    end = add_months(sold, months)
    shop = w.r.choice(SHOPS)
    s = Sheet(*A4, margin=120)
    s.text(f"{shop} S.R.L.", 34, True)
    s.text(f"CUI RO{w.num(8)}, J40/{w.num(5)}/{w.r.randint(2000, 2020)}", 24, color=(90, 90, 90))
    s.space(40)
    s.text("CERTIFICAT DE GARANȚIE", 48, True, align="center")
    s.text(f"Nr. {w.num(8)}", 30, align="center")
    s.space(30)
    s.pair("Produs:", f"{product} {brand}")
    s.pair("Model:", model)
    s.pair("Serie produs:", f"{w.r.choice(['SN', 'S/N '])}{w.num(4)}{w.r.choice('ABCDEFGH')}{w.num(6)}")
    s.pair("Cumpărător:", f"{p['first']} {p['last']}")
    s.pair("Data vânzării:", ro_date(sold))
    s.pair("Factura nr.:", f"{w.r.choice(['EMG', 'ALX', 'FLN', 'DDM', 'MGX'])}{w.num(8)} din {ro_date(sold)}")
    s.pair("Termen de garanție:", f"{months} de luni")
    s.space(30)
    s.text("Condiții de garanție", 30, True)
    for t in WARRANTY_TERMS:
        s.para(t)
    s.space(40)
    s.text("Semnătura și ștampila vânzătorului", 24, color=(90, 90, 90))
    s.stamp(1250, s.y + 80, 110)
    short = product.split()[0].lower() if product.split()[0] not in ("Mașină", "Cuptor", "Aspirator") else " ".join(product.lower().split()[:3 if product.startswith("Mașină") else 2])
    fields = {"product": f"{product} {brand}", "seller": shop}
    deadlines = [{"kind": "warranty_end", "date": end.isoformat()}]
    q = [{"question": f"Când expiră garanția la {short}?", "page": 1, "date": end.isoformat()},
         {"question": f"De unde am cumpărat {short.split()[0]} {brand}?", "page": 1, "text": shop.split()[0]}]
    return Doc(f"warranty-{i}", "warranty", [s], fields, deadlines, q)


def rent(w: World, i: int, landlord: dict, tenant: dict) -> Doc:
    signed = w.day_between(TODAY - dt.timedelta(days=360), TODAY - dt.timedelta(days=5))
    start = dt.date(signed.year + (signed.month == 12), signed.month % 12 + 1, 1)
    months = w.r.choice([12, 24])
    end = add_months(start, months) - dt.timedelta(days=1)
    notice = w.r.choice([30, 60])
    rent_eur = w.r.choice([350, 420, 480, 550, 650])
    street = w.r.choice(STREETS)
    address = f"{street} nr. {w.r.randint(1, 90)}, bl. {w.r.choice('ABCDEFG')}{w.r.randint(1, 9)}, ap. {w.r.randint(1, 60)}, {tenant['city']}"
    p1 = Sheet(*A4, margin=130)
    p1.text(f"CONTRACT DE ÎNCHIRIERE nr. {w.r.randint(1, 99)} din {ro_date(signed)}", 36, True, align="center", family="serif")
    p1.space(30)
    p1.text("I. PĂRȚILE CONTRACTANTE", 30, True, family="serif")
    p1.para(f"1.1. {landlord['last'].upper()} {landlord['first'].upper()}, domiciliat(ă) în {landlord['address']}, în calitate de LOCATOR, și")
    p1.para(f"1.2. {tenant['last'].upper()} {tenant['first'].upper()}, domiciliat(ă) în {tenant['address']}, în calitate de LOCATAR,")
    p1.para("au convenit încheierea prezentului contract de închiriere, cu respectarea următoarelor clauze:")
    p1.text("II. OBIECTUL CONTRACTULUI", 30, True, family="serif")
    p1.para(f"2.1. Locatorul închiriază locatarului apartamentul situat în {address}, compus din 2 camere și dependințe, mobilat și utilat.")
    p1.text("III. DURATA CONTRACTULUI", 30, True, family="serif")
    p1.para(f"3.1. Contractul se încheie pe o perioadă de {months} de luni, începând cu data de {ro_date(start)}.")
    p1.para("3.2. La expirarea termenului, contractul poate fi prelungit prin act adițional semnat de ambele părți.")
    p1.text("IV. CHIRIA ȘI MODALITATEA DE PLATĂ", 30, True, family="serif")
    p1.para(f"4.1. Chiria lunară este de {rent_eur} euro, plătibilă în lei la cursul BNR din ziua plății, până la data de 5 a fiecărei luni.")
    p1.para(f"4.2. La semnarea contractului locatarul achită o garanție de {rent_eur} euro, care se restituie la încetarea contractului.")
    p2 = Sheet(*A4, margin=130)
    p2.text("V. OBLIGAȚIILE PĂRȚILOR", 30, True, family="serif")
    p2.para("5.1. Locatorul se obligă să predea locuința în stare normală de folosință și să asigure folosința liniștită a acesteia pe toată durata contractului.")
    p2.para("5.2. Locatarul se obligă să plătească la termen chiria și cheltuielile de întreținere, energie electrică, gaze și internet.")
    p2.text("VI. ÎNCETAREA CONTRACTULUI", 30, True, family="serif")
    p2.para(f"6.1. Oricare dintre părți poate denunța unilateral contractul cu un preaviz de {notice} de zile, notificat în scris.")
    p2.para("6.2. Contractul încetează de drept la expirarea duratei pentru care a fost încheiat, dacă nu este prelungit.")
    p2.text("VII. DISPOZIȚII FINALE", 30, True, family="serif")
    p2.para("7.1. Prezentul contract a fost încheiat în două exemplare originale, câte unul pentru fiecare parte, și va fi înregistrat la ANAF.")
    p2.space(80)
    p2.text("LOCATOR                                              LOCATAR", 28, True, family="serif")
    fields = {"seller": f"{landlord['first']} {landlord['last']}", "address": address, "amount": f"{rent_eur} euro"}
    deadlines = [{"kind": "contract_end", "date": end.isoformat(), "notice_days": notice}]
    q = [{"question": f"Câtă chirie plătesc pentru apartamentul de pe {street}?", "page": 1, "text": str(rent_eur)},
         {"question": f"Cu câte zile înainte trebuie să anunț dacă vreau să mă mut din apartamentul de pe {street}?", "page": 2, "text": str(notice)},
         {"question": f"Când se termină contractul de închiriere pentru apartamentul de pe {street}?", "page": 1, "date": end.isoformat()}]
    return Doc(f"rent-{i}", "rent_contract", [p1, p2], fields, deadlines, q)


def invoice(w: World, i: int, p: dict) -> Doc:
    supplier, what, code = SUPPLIERS[i % len(SUPPLIERS)]
    # One bill a month per supplier: the fifth bill (the second from the first supplier) is a month older.
    back = (i // len(SUPPLIERS)) * 31
    issued = w.day_between(TODAY - dt.timedelta(days=28 + back), TODAY - dt.timedelta(days=2 + back))
    due = issued + dt.timedelta(days=w.r.choice([15, 20, 30]))
    period_end = issued - dt.timedelta(days=w.r.randint(1, 5))
    period_start = add_months(period_end, -1) + dt.timedelta(days=1)
    amount = f"{w.r.randint(45, 520)},{w.r.randint(0, 99):02d}"
    s = Sheet(*A4, margin=110)
    s.band((230, 120, 20) if code in ("ENM", "EGR") else (20, 90, 170), 60)
    s.space(40)
    s.text(supplier, 34, True)
    s.text(f"Furnizor de {what}", 26, color=(90, 90, 90))
    s.space(30)
    s.text("FACTURĂ FISCALĂ", 46, True, align="center")
    s.text(f"Seria {code} nr. {w.num(8)}", 30, align="center")
    s.space(30)
    s.pair("Client:", f"{p['first']} {p['last']}")
    s.pair("Adresa locului de consum:", p["address"])
    s.pair("Cod client:", w.num(10))
    s.pair("Data facturii:", ro_date(issued))
    s.pair("Perioada de facturare:", f"{ro_date(period_start)} - {ro_date(period_end)}")
    s.rule()
    s.pair("Valoare fără TVA:", f"{amount} lei")
    s.pair("Total de plată:", f"{amount} lei")
    s.pair("Data scadenței:", ro_date(due))
    s.rule()
    s.para("Plata se poate face online, prin internet banking, la bancomatele partenere sau în magazinele cu serviciul de încasare facturi. Neplata la termen atrage penalități de 0,04% pe zi.", family="sans")
    month = MONTHS_RO[issued.month - 1]
    fields = {"seller": supplier, "amount": f"{amount} lei"}
    deadlines = [{"kind": "invoice_due", "date": due.isoformat()}]
    q = [{"question": f"Până când trebuie plătită factura de la {supplier.split()[0]} din {month}?", "page": 1, "date": due.isoformat()},
         {"question": f"Cât am de plată la factura {supplier.split()[0]} din {month}?", "page": 1, "text": amount}]
    return Doc(f"invoice-{i}", "invoice", [s], fields, deadlines, q)


def build(seed: int) -> list[Doc]:
    w = World(seed)
    docs: list[Doc] = []
    for i, car in enumerate(w.cars):
        docs.append(rca(w, i + 1, car))
        docs.append(talon(w, i + 1, car))
    first = dt.date.fromisoformat(docs[0].deadlines[0]["date"])
    first_start = add_months(first + dt.timedelta(days=1), -12) if docs[0].deadlines else TODAY
    docs.append(rca(w, 4, w.cars[0], before=first_start))  # the same car's previous, expired policy
    docs.append(itp_report(w, 1, w.cars[0]))
    docs.append(itp_report(w, 2, w.cars[1]))
    docs.append(casco(w, 1, w.cars[0]))
    docs.append(casco(w, 2, w.cars[2]))
    for i, p in enumerate(w.people[:2]):
        docs.append(passport(w, i + 1, p))
        docs.append(id_card(w, i + 1, p))
        docs.append(licence(w, i + 1, p))
    for i in range(5):
        docs.append(warranty(w, i, w.people[0]))
    docs.append(rent(w, 1, w.people[3], w.people[0]))
    docs.append(rent(w, 2, w.people[2], w.people[1]))
    for i in range(5):
        docs.append(invoice(w, i, w.people[0]))
    # A third of the documents are flat scans; the rest are photos.
    for k, d in enumerate(docs):
        d.flat = k % 3 == 2
    # The expired RCA: the questions about that car are about the policy in force.
    docs[6].questions = []
    return docs


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", default="../testdata/synthetic")
    ap.add_argument("--seed", type=int, default=12)
    a = ap.parse_args()
    out = Path(a.out)
    (out / "photos").mkdir(parents=True, exist_ok=True)
    (out / "clean").mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(a.seed)
    docs = build(a.seed)
    truth = {"today": TODAY.isoformat(), "seed": a.seed, "documents": [], "questions": []}
    for d in docs:
        pages = []
        for n, sheet in enumerate(d.pages, 1):
            name = f"{d.id}-p{n}"
            sheet.img.save(out / "clean" / f"{name}.png", optimize=True)
            jpeg, corners, size = photograph(sheet.img, rng, flat=d.flat)
            (out / "photos" / f"{name}.jpg").write_bytes(jpeg)
            pages.append({"photo": f"photos/{name}.jpg", "clean": f"clean/{name}.png", "corners": corners,
                          "size": list(size), "page_size": list(sheet.img.size), "text": "\n".join(sheet.lines)})
        truth["documents"].append({"id": d.id, "kind": d.kind, "flat": d.flat, "fields": d.fields, "deadlines": d.deadlines, "pages": pages})
        for q in d.questions:
            # Another document that states the same deadline (the ITP report of a car whose
            # registration has the ITP stamp) is as good a source.
            also = [o.id for o in docs if o is not d and "date" in q
                    and any(x["date"] == q["date"] and x["kind"] == dl["kind"] for x in o.deadlines for dl in d.deadlines)]
            truth["questions"].append({"document": d.id, **q, **({"also": also} if also else {})})
    (out / "truth.json").write_text(json.dumps(truth, indent=1, ensure_ascii=False), encoding="utf-8", newline="\n")
    print(f"wrote {len(docs)} documents ({sum(len(d.pages) for d in docs)} pages) and {len(truth['questions'])} questions to {out}")


if __name__ == "__main__":
    main()
