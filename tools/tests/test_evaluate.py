import unittest

from docket_tools import evaluate as ev


class ScoringTest(unittest.TestCase):
    def test_cer_ignores_spacing_and_cedilla_letters(self):
        self.assertEqual(ev.cer("Poliță  RCA\nvalabilă", "Poliţă RCA valabilă"), 0.0)
        self.assertAlmostEqual(ev.cer("abcd", "abed"), 0.25)
        self.assertAlmostEqual(ev.wer("până la 14.03.2027", "pana la 14.03.2027"), 1 / 3)

    def test_answers_are_matched_by_date_in_any_form(self):
        q = {"date": "2027-03-14"}
        for text in ["Expiră pe 14.03.2027.", "pe 14 martie 2027", "2027-03-14", "expira la 14/03/2027"]:
            self.assertTrue(ev.answer_ok(q, text), text)
        self.assertFalse(ev.answer_ok(q, "Expiră pe 13.03.2027."))
        self.assertTrue(ev.answer_ok({"text": "eMAG"}, "Ai cumpărat-o de la EMAG."))

    def test_fields_and_inventions(self):
        fields = [{"key": "plate", "value": "CT 94 THC"}]
        self.assertTrue(ev.field_found("CT-94-THC", fields))
        self.assertFalse(ev.field_found("CT-94-JMM", fields))
        got = {"fields": {"plate": {"value": "B-123-ABC"}, "vin": {"value": "UU1ABC"}}}
        self.assertEqual(ev.invented(got, "Nr. înmatriculare UU1ABC"), 1)

    def test_extraction_score(self):
        doc = {"kind": "rca", "fields": {"plate": "CT-94-THC"}, "deadlines": [{"kind": "rca_expiry", "date": "2027-03-02"}]}
        got = {"kind": "rca", "fields": [{"key": "plate", "value": "CT 94 THC"}],
               "deadlines": [{"kind": "rca_expiry", "date": "2027-03-02"}, {"kind": "other", "date": "2026-01-01"}],
               "extraction": {"rejected": ["x"], "seconds": 1.5}}
        s = ev.score_extraction(doc, got)
        self.assertTrue(s["kind"])
        self.assertEqual((s["deadlines"], s["extra_deadlines"], s["fields"], s["rejected"]), (1.0, 1, 1.0, 1))


if __name__ == "__main__":
    unittest.main()
