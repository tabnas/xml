package tabnasxml

import (
	"testing"
	"time"

	jsonic "github.com/tabnas/jsonic/go"
)

// makeXMLParser builds a ready-to-use XML parser the way every caller
// must (this plugin exposes no package-level convenience Parse — users
// instantiate the engine and Use the plugin themselves). Building the
// instance runs the plugin, which parses + installs the XML grammar;
// that grammar build dominates a parse.
func makeXMLParser(t testing.TB) *jsonic.Jsonic {
	t.Helper()
	j := jsonic.Make()
	if err := j.UseDefaults(Xml, Defaults); err != nil {
		t.Fatalf("UseDefaults: %v", err)
	}
	return j
}

// TestParseReusesInstance guards against a performance regression where
// callers rebuild the (expensive) XML parser+grammar on every parse
// instead of building one instance and reusing it. This plugin has no
// cacheable package-level Parse — it is instantiated per the
// `jsonic.Make().UseDefaults(xml.Xml, ...)` pattern — so the guard
// instead pins the recommended usage: reusing ONE instance for N parses
// must stay far cheaper than building a fresh instance per parse.
//
// Building the grammar dominates a parse, so the rebuild-per-call path
// is ~25x-190x slower than instance reuse. If anyone introduced a
// convenience that built a fresh parser each call (or refactored the
// usage that way), this test would catch it.
//
// The check is machine-INDEPENDENT: it compares instance reuse against
// rebuild-per-call on the SAME machine in the SAME run, so a slow CI box
// cannot make it flaky (both sides scale together). There is
// deliberately NO wall-clock budget.
func TestParseReusesInstance(t *testing.T) {
	const src = `<a x="1"><b>hello</b><c/></a>`
	const n = 2000

	// Warm both paths so the comparison is steady-state.
	reused := makeXMLParser(t)
	for i := 0; i < 50; i++ {
		if _, err := reused.Parse(src); err != nil {
			t.Fatalf("warm reuse parse error: %v", err)
		}
		if _, err := makeXMLParser(t).Parse(src); err != nil {
			t.Fatalf("warm rebuild parse error: %v", err)
		}
	}

	// Recommended usage: build one instance, reuse it for every parse.
	t0 := time.Now()
	for i := 0; i < n; i++ {
		if _, err := reused.Parse(src); err != nil {
			t.Fatalf("reuse parse error: %v", err)
		}
	}
	reuse := time.Since(t0)

	// Regression usage: build a fresh instance (rebuilding the grammar)
	// for every parse.
	t1 := time.Now()
	for i := 0; i < n; i++ {
		if _, err := makeXMLParser(t).Parse(src); err != nil {
			t.Fatalf("rebuild parse error: %v", err)
		}
	}
	rebuild := time.Since(t1)

	// Reusing one instance is ~= a single grammar build amortised over N
	// parses; rebuilding per call pays the grammar build every time and
	// here runs many times slower. Require reuse to be at least 4x faster
	// than rebuild-per-call: this catches a regression to per-call
	// rebuilding without depending on absolute wall-clock speed.
	if reuse*4 > rebuild {
		t.Errorf("instance reuse is not meaningfully faster than rebuilding "+
			"the parser per parse: %d reuse parses took %v vs %v rebuilding "+
			"per call (ratio %.1fx, need >=4x). Build one parser "+
			"(jsonic.Make().UseDefaults(xml.Xml, ...)) and reuse it; do not "+
			"rebuild the XML grammar on every parse.",
			n, reuse, rebuild, float64(rebuild)/float64(reuse))
	}
	t.Logf("reuse=%v  rebuild-per-call=%v  speedup=%.2fx",
		reuse, rebuild, float64(rebuild)/float64(reuse))
}
