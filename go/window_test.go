package tabnasxml

// A parse from a reader, a window of the source at a time (admin ADR-30;
// tabnas/parser's doc/chunked-source.md, "Proving it"), against the same
// text parsed whole. Make declares Lex.Window: the plugin's matcher reads
// the text only through the lexer's window, and reads the window on where a
// construct runs past its end, so a reader is read a window at a time.
//
// Every row of the shared fixtures and every document of the W3C XML
// Conformance Test Suite is parsed whole and from a reader that hands its
// bytes over in pieces of 1, 3 and 64 bytes. The engine's window sizes are
// fixed in Go (only its own tests can make them a few bytes), and the
// window reaches 64 KiB past the cursor before each token is cut, so the
// edge is proved with long documents: their tokens of every kind straddle
// the reads, and the long ones, longer than the window, run past its end,
// in pieces of 1, 3, 64 and 4096 bytes. The value, the tokens the lexer
// gives, the rule passes, and an error's code, position, row, column and
// token must be the same. An error's excerpt is not compared: a parse from
// a reader shows a long line only as far back as the window reaches
// (tabnas/parser's DIVERGENCE.md).
//
// ts/test/window.test.ts and rs/tests/window_test.rs hold the TypeScript
// and Rust ports to the same, under windows of a few units too.

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"testing"
	"unicode/utf8"

	support "github.com/tabnas/support/go"

	jsonic "github.com/tabnas/jsonic/go"
	tabnas "github.com/tabnas/parser/go"
)

// pieces is a reader that hands its bytes over at most size at a time.
type pieces struct {
	bytes []byte
	at    int
	size  int
}

func (p *pieces) Read(buf []byte) (int, error) {
	if p.at >= len(p.bytes) {
		return 0, io.EOF
	}
	n := min(p.size, len(buf), len(p.bytes)-p.at)
	copy(buf, p.bytes[p.at:p.at+n])
	p.at += n
	return n, nil
}

// readSizes are the pieces a reader hands a fixture row or a conformance
// document over in, and longSizes a long document's. A conformance document
// longer than 16 KiB is read in pieces of 64 bytes and 4 KiB: the Go engine
// copies its window at each read (tabnas/parser#321), so a read of a few
// bytes costs the window's length, and a long document read a byte at a
// time takes seconds where its parse takes milliseconds.
var (
	readSizes = []int{1, 3, 64}
	bigSizes  = []int{64, 4096}
	longSizes = []int{61, 4096, 65536}
)

// windowLog gathers the tokens the lexer gives and the rule passes.
type windowLog struct {
	tokens []string
	rules  []string
}

// logged subscribes log to j's tokens and rule passes.
func logged(j *tabnas.Tabnas, log *windowLog) *tabnas.Tabnas {
	j.Sub(func(tkn *tabnas.Token, _ *tabnas.Rule, _ *tabnas.Context) {
		val, _ := json.Marshal(tkn.Val)
		log.tokens = append(log.tokens, fmt.Sprintf("%s %q %s %d %d:%d",
			tkn.Name, tkn.Src, val, tkn.SI, tkn.RI, tkn.CI))
	}, nil)
	j.SubRuleDone(func(r *tabnas.Rule, _ *tabnas.Context, done tabnas.RuleDone) {
		log.rules = append(log.rules, fmt.Sprintf("%s %v %d", r.Name, done.State, r.D))
	})
	return j
}

// seenParse is what a parse showed: its value, or its error without the
// excerpt; and the tokens and rule passes the log gathered.
type seenParse struct {
	outcome string
	tokens  []string
	rules   []string
}

// outcome is a parse's value as JSON, or its error's code, position, row,
// column and token.
func outcome(value any, err error) string {
	if err != nil {
		var te *tabnas.TabnasError
		if errors.As(err, &te) {
			return fmt.Sprintf("err %s at %d %d:%d %q", te.Code, te.Pos, te.Row, te.Col, te.Src)
		}
		return "fail " + err.Error()
	}
	text, err := json.Marshal(value)
	if err != nil {
		return "unmarshallable " + err.Error()
	}
	return "ok " + string(text)
}

// seen takes what log gathered, which is empty again after.
func seen(log *windowLog, value any, err error) seenParse {
	got := seenParse{outcome: outcome(value, err), tokens: log.tokens, rules: log.rules}
	log.tokens, log.rules = nil, nil
	return got
}

// differential parses text whole and from a reader in each of sizes, and
// fails t where the two differ. It returns the count of reader parses
// compared.
func differential(t *testing.T, j *tabnas.Tabnas, log *windowLog, at, text string, sizes []int) int {
	t.Helper()
	log.tokens, log.rules = nil, nil
	value, err := j.Parse(text)
	whole := seen(log, value, err)
	for _, size := range sizes {
		value, err := j.ParseReader(&pieces{bytes: []byte(text), size: size})
		read := seen(log, value, err)
		if read.outcome != whole.outcome {
			t.Fatalf("%s in pieces of %d:\nread  %.300s\nwhole %.300s", at, size, read.outcome, whole.outcome)
		}
		if !reflect.DeepEqual(read.tokens, whole.tokens) {
			t.Fatalf("%s in pieces of %d: the tokens differ\nread  %.400v\nwhole %.400v", at, size, read.tokens, whole.tokens)
		}
		if !reflect.DeepEqual(read.rules, whole.rules) {
			t.Fatalf("%s in pieces of %d: the rule passes differ", at, size)
		}
	}
	return len(sizes)
}

// rowOptions are a fixture row's plugin options.
func rowOptions(t *testing.T, row *support.Row) map[string]any {
	t.Helper()
	opts := map[string]any{}
	if raw := row.Named("opts"); "" != strings.TrimSpace(raw) {
		if err := json.Unmarshal([]byte(raw), &opts); err != nil {
			t.Fatalf("%s: the opts column is not JSON: %v", row.Where(), err)
		}
	}
	return opts
}

// fixtureRows are the shared fixtures' rows.
func fixtureRows(t *testing.T) []*support.Row {
	t.Helper()
	dir, err := support.FindSpecDir("")
	if err != nil {
		t.Fatal(err)
	}
	files, err := support.LoadSpecDir(dir, nil)
	if err != nil {
		t.Fatal(err)
	}
	var rows []*support.Row
	for _, file := range files {
		rows = append(rows, file.Rows...)
	}
	return rows
}

// conformanceDocuments are the texts of every .xml file of the W3C
// corpus, by path. The corpus mixes UTF-8, UTF-16 and UTF-32; DecodeBOM
// transcodes, as the conformance suite reads each document, and a byte
// that is not UTF-8 is read as U+FFFD, as a reader of the same text gives
// none.
func conformanceDocuments(t *testing.T) ([]string, map[string]string) {
	t.Helper()
	root := xmlconfRoot(t)
	var paths []string
	err := filepath.WalkDir(root, func(path string, entry os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if !entry.IsDir() && strings.HasSuffix(path, ".xml") {
			paths = append(paths, path)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	sort.Strings(paths)
	texts := map[string]string{}
	for _, path := range paths {
		body, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		text := DecodeBOM(string(body))
		if !utf8.ValidString(text) {
			text = strings.ToValidUTF8(text, "\uFFFD")
		}
		texts[path] = text
	}
	return paths, texts
}

// longDocument is a document of every kind of token, the long ones longer
// than the engine's 64 KiB window, and characters past ASCII among them, so
// that tokens of every kind straddle the window's edges as a reader hands
// the text over. The same document as longDocument in
// ts/test/window.test.ts and long_document in rs/tests/window_test.rs.
func longDocument() string {
	var b strings.Builder
	b.WriteString("\uFEFF<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n")
	b.WriteString("<!DOCTYPE doc [\n")
	b.WriteString("  <!ENTITY who \"w\u00f6rld\">\n  <!ATTLIST x:item kind CDATA \"plain\">\n")
	for i := 0; i < 1300; i++ {
		fmt.Fprintf(&b, "  <!ENTITY e%d \"entity \u20ac %d\"> <!-- %d > ] ' -->\n", i, i, i)
	}
	b.WriteString("]>\n")
	b.WriteString("<!-- " + strings.Repeat("a comment past the window, \u00e9 \U0001d11e - ", 2000) + "-->\n")
	b.WriteString("<doc xmlns=\"urn:doc\" xmlns:x=\"urn:x\" note=\"" +
		strings.Repeat("an attribute value past the window &amp; \u00fc ", 1600) + "\">\n")
	for i := 0; i < 1000; i++ {
		fmt.Fprintf(&b, "  <x:item n=\"%d\" x:at='%d'>&who; &#%d; &#x%x; t\u00ebxt %d</x:item>\r\n",
			i, i%7, 60+i%40, 0x4e00+i, i)
		if i == 500 {
			b.WriteString("  <![CDATA[" + strings.Repeat("cdata <b>not markup</b> \u00e7 ]] > ", 2200) + "]]>\n")
			b.WriteString("  <?pi " + strings.Repeat("an instruction past the window \u00f1 ? > ", 1800) + "?>\n")
			b.WriteString("  <text>" +
				strings.Repeat("character data past the window, \u00e5 &amp; &lt;x&gt; \r\n", 1300) + "</text>\n")
			b.WriteString("  <e" + strings.Repeat("n", 70000) + "/>\n")
			b.WriteString("  <empty   a=\"1\"\n     b = \"2\"  />\n")
		}
	}
	b.WriteString("</doc>\n<!-- after -->\n")
	return b.String()
}

func TestMakeReadsAReaderAWindowAtATime(t *testing.T) {
	declares := func(j *tabnas.Tabnas) bool {
		lex := j.Options().Lex
		return lex != nil && lex.Window != nil && *lex.Window
	}
	for _, options := range []map[string]any{nil, {"strictNamespaces": true}} {
		j, err := Make(options)
		if err != nil {
			t.Fatal(err)
		}
		if !declares(j) {
			t.Fatalf("Make(%v) does not declare Lex.Window", options)
		}
	}
	// The plugin itself declares nothing: a grammar built on it (feed)
	// declares the window on its own parser, once its own actions are
	// verified.
	j := tabnas.Make()
	if err := j.UseDefaults(Xml, Defaults); err != nil {
		t.Fatal(err)
	}
	if declares(j) {
		t.Fatal("the plugin declares Lex.Window")
	}
}

func TestMakeRefusesEmbedMode(t *testing.T) {
	if _, err := Make(map[string]any{"embed": true}); !errors.Is(err, errEmbedNeedsJsonic) {
		t.Fatalf("Make in embed mode: %v", err)
	}
}

// Make builds on the bare engine, as TypeScript's make and Rust's
// make_with do, where the Go suite and dmog built the parser on a jsonic
// engine: every fixture row and every conformance document parses to the
// same value or error on both.
func TestMakeParsesAsAJsonicEngineDoes(t *testing.T) {
	parsers := func(options map[string]any) (*tabnas.Tabnas, *tabnas.Tabnas) {
		made, err := Make(options)
		if err != nil {
			t.Fatal(err)
		}
		hosted := jsonic.Make()
		if err := hosted.UseDefaults(Xml, Defaults, options); err != nil {
			t.Fatal(err)
		}
		return made, hosted
	}
	compare := func(at string, made, hosted *tabnas.Tabnas, text string) {
		got, want := outcome(made.Parse(text)), outcome(hosted.Parse(text))
		if got != want {
			t.Fatalf("%s:\nMake   %.300s\njsonic %.300s", at, got, want)
		}
	}
	rows := fixtureRows(t)
	for _, row := range rows {
		made, hosted := parsers(rowOptions(t, row))
		compare(row.Where(), made, hosted, specUnescape(row.Named("input")))
	}
	made, hosted := parsers(nil)
	paths, texts := conformanceDocuments(t)
	for _, path := range paths {
		compare(path, made, hosted, texts[path])
	}
	t.Logf("%d fixture rows and %d conformance documents", len(rows), len(paths))
}

func TestEveryFixtureRowParsesTheSameFromAReader(t *testing.T) {
	log := &windowLog{}
	rows := fixtureRows(t)
	if len(rows) < 200 {
		t.Fatalf("%d fixture rows", len(rows))
	}
	compared := 0
	for _, row := range rows {
		j, err := Make(rowOptions(t, row))
		if err != nil {
			t.Fatal(err)
		}
		compared += differential(t, logged(j, log), log, row.Where(), specUnescape(row.Named("input")), readSizes)
	}
	if compared != len(rows)*len(readSizes) {
		t.Fatalf("%d reader parses compared", compared)
	}
}

func TestEmbedModeOnAJsonicEngineReadsAReaderAWindowAtATimeToo(t *testing.T) {
	// Embed mode is built on jsonic's engine, which declares no window, so
	// the window is declared here: the plugin's matcher is the same in both
	// modes, and is held to the same.
	log := &windowLog{}
	j := jsonic.Make()
	if err := j.UseDefaults(Xml, Defaults, map[string]any{"embed": true}); err != nil {
		t.Fatal(err)
	}
	window := true
	j.SetOptions(tabnas.Options{Lex: &tabnas.LexOptions{Window: &window}})
	logged(j, log)
	for _, input := range []string{
		`{a: <x>1, 2</x>, b: [<y/>, <z q="r">s</z>]}`,
		`a: <p>T&amp;J <b>bold</b> <![CDATA[<raw>]]> <!-- c --></p>`,
		`[<a/>, <b></b>, 3]`,
		`{a: <x>unclosed}`,
		`{a: <x></y>}`,
	} {
		differential(t, j, log, input, input, readSizes)
	}
}

func TestAByteOrderMarkIsDroppedAtTheSourcesStartNotTheWindows(t *testing.T) {
	// A byte order mark is the source's first character, and a U+FEFF
	// anywhere else is character data, as in the whole text, however much
	// of the source the window has let go of: here a block of 64 KiB lies
	// behind it, and the parser keeps no consumed token for a rewind. (The
	// Go engine keeps the last two consumed tokens all the same, so its
	// window does not begin at a cursor past the source's start, which
	// TypeScript's can: ts/test/window.test.ts makes it.)
	log := &windowLog{}
	j, err := Make()
	if err != nil {
		t.Fatal(err)
	}
	none := 0
	j.SetOptions(tabnas.Options{Rewind: &tabnas.RewindOptions{History: &none}})
	logged(j, log)
	long := "<p>" + strings.Repeat("x", 70000) + "</p>"
	for _, input := range []string{
		"<a>" + long + "\uFEFFx</a>",
		"<a>" + long + "<b/>\uFEFFx</a>",
		"\uFEFF<a>" + long + "<b/>\uFEFFx</a>",
	} {
		differential(t, j, log, fmt.Sprintf("%.20q", input), input, longSizes)
	}
}

func TestEveryConformanceDocumentParsesTheSameFromAReader(t *testing.T) {
	log := &windowLog{}
	j, err := Make()
	if err != nil {
		t.Fatal(err)
	}
	logged(j, log)
	paths, texts := conformanceDocuments(t)
	if len(paths) < 3000 {
		t.Fatalf("%d conformance documents", len(paths))
	}
	compared := 0
	for index, path := range paths {
		sizes := readSizes
		if 16<<10 < len(texts[path]) {
			sizes = bigSizes
		}
		compared += differential(t, j, log, path, texts[path], sizes)
		if index%500 == 0 || index+1 == len(paths) {
			t.Logf("window conformance: %d of %d documents (%d%%), %d reader parses",
				index+1, len(paths), (index+1)*100/len(paths), compared)
		}
	}
}

func TestALongDocumentParsesTheSameFromAReader(t *testing.T) {
	log := &windowLog{}
	j, err := Make()
	if err != nil {
		t.Fatal(err)
	}
	text := longDocument()
	if len(text) < 512*1024 {
		t.Fatalf("the long document is %d bytes", len(text))
	}
	value, err := j.Parse(text)
	if err != nil {
		t.Fatal(err)
	}
	if doc, _ := value.(map[string]any); doc["name"] != "doc" {
		t.Fatalf("the long document parses to %.80v", value)
	}
	differential(t, logged(j, log), log, "the long document", text, longSizes)
}

func TestALongDocumentFailsTheSameFromAReader(t *testing.T) {
	// A construct that runs past the engine's window and is then refused,
	// or never ends: the error is the whole text's, code, place and token.
	log := &windowLog{}
	j, err := Make()
	if err != nil {
		t.Fatal(err)
	}
	logged(j, log)
	pad := "<p>" + strings.Repeat("x\u00e9 ", 30000) + "</p>"
	bad := []string{
		"<a>" + pad + "<!-- " + strings.Repeat("c ", 40000),
		"<a>" + pad + "<![CDATA[" + strings.Repeat("d ", 40000),
		"<a>" + pad + "<?pi " + strings.Repeat("p ", 40000),
		"<!DOCTYPE a [" + strings.Repeat(`<!ENTITY e "v">`, 6000) + "<a>" + pad + "</a>",
		"<a>" + pad + strings.Repeat("t ", 40000) + "]]>" + strings.Repeat("t ", 10) + "</a>",
		"<a>" + pad + `<b c="` + strings.Repeat("v ", 40000) + `<"/></a>`,
		"<a>" + pad + `<b c="` + strings.Repeat("v ", 40000),
		"<a>" + pad + "<!-- " + strings.Repeat("c ", 40000) + "-- -->",
		"<a>" + pad + "</b>",
		"<a>" + pad + "</a>" + strings.Repeat("trailing ", 10000),
	}
	for index, text := range bad {
		if _, err := j.Parse(text); err == nil {
			t.Fatalf("long error document %d parses", index)
		}
		log.tokens, log.rules = nil, nil
		differential(t, j, log, fmt.Sprintf("long error document %d", index), text, longSizes)
	}
}
