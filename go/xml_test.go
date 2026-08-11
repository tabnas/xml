package tabnasxml

import (
	"encoding/json"
	"fmt"
	"regexp"
	"strconv"
	"strings"
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
	support "github.com/tabnas/support/go"
)

// ansiRE matches the SGR colour sequences the engine writes into
// rendered error messages, so the `msg` spec column can be plain text.
var ansiRE = regexp.MustCompile("\x1b\\[[0-9;]*m")

func stripANSI(s string) string { return ansiRE.ReplaceAllString(s, "") }

// asMap returns the string-keyed map underlying a parsed object,
// accepting both the insertion-ordered *jsonic.OrderedMap that the parser
// now produces for JSON/Jsonic objects and a plain map[string]any (which
// the XML plugin still uses for element nodes it builds itself). It lets
// value assertions work regardless of which shape a given node is.
func asMap(v any) map[string]any {
	if om, ok := v.(*jsonic.OrderedMap); ok {
		return om.Vals
	}
	m, _ := v.(map[string]any)
	return m
}

// specEntry represents one row of a TSV spec file.
// TestSpec runs every fixture in the spec directory. FindSpecDir walks up
// from the package directory, and Dir discovers the files by listing, so
// adding a .tsv runs it in both runtimes without touching either runner.
//
// A row is `# name<TAB>input<TAB>expected<TAB>opts<TAB>msg`. The header
// line begins with #, which is why the columns are read by NAME and why
// the first one is called "# name".
func TestSpec(t *testing.T) {
	dir, err := support.FindSpecDir("")
	if err != nil {
		t.Fatal(err)
	}

	support.Runner{
		// The runner's own decoding of the input column is bypassed — see
		// specUnescape below — so the raw cell is read and decoded here.
		ParseRow: func(_ string, row *support.Row) (any, error) {
			input := specUnescape(row.Named("input"))

			opts := map[string]any{}
			if raw := row.Named("opts"); "" != strings.TrimSpace(raw) {
				if err := json.Unmarshal([]byte(raw), &opts); err != nil {
					return nil, err
				}
			}

			j := jsonic.Make()
			if err := j.UseDefaults(Xml, Defaults, opts); err != nil {
				return nil, err
			}
			return j.Parse(input)
		},

		// Two things the default code comparison does not do. The engine
		// renders a code as jsonic/<code>, so both spellings are accepted;
		// and the optional msg column pins the rendered message, so a
		// template that stops interpolating — leaving a literal
		// placeholder behind — fails here rather than silently shipping.
		MatchError: func(err error, want string, row *support.Row) bool {
			message := err.Error()
			if !strings.Contains(message, want) {
				return false
			}
			msg := row.Named("msg")
			return "" == msg || strings.Contains(stripANSI(message), msg)
		},

		// Flatten through JSON so []any versus a concrete slice type, and
		// Go's numeric types, compare against the fixture's decoded shape.
		Normalize: jsonFlatten,

		InputName:    "input",
		ExpectedName: "expected",
		CaseName: func(row *support.Row, _ string) string {
			return fmt.Sprintf("row %d: %s", row.Line, row.Named("# name"))
		},
	}.Dir(t, dir)
}

// specUnescape is the one thing this repo does not take from the support
// module: its own escape codec, because xml's fixtures need a sixth
// escape.
//
// \uXXXX names a character that must not be written literally into a
// fixture — a leading U+FEFF byte-order mark above all, which is invisible
// in a diff. The shared codec passes \u through on purpose: an XML
// fixture, like a JSON one, has to be able to carry a literal \u0041 as
// source text. So it is decoded here, in one pass over the RAW cell; after
// the shared codec an escaped backslash followed by uFEFF is
// indistinguishable from a plain \uFEFF.
//
// Kept byte-identical to unescapeInput in ts/test/xml-spec.test.ts.
func specUnescape(s string) string {
	if !strings.Contains(s, `\`) {
		return s
	}
	var b strings.Builder
	b.Grow(len(s))
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c == '\\' && i+1 < len(s) {
			switch s[i+1] {
			case 'n':
				b.WriteByte('\n')
				i++
				continue
			case 'r':
				b.WriteByte('\r')
				i++
				continue
			case 't':
				b.WriteByte('\t')
				i++
				continue
			case '\\':
				b.WriteByte('\\')
				i++
				continue
			case 'u':
				if i+5 < len(s) {
					if cp, err := strconv.ParseUint(s[i+2:i+6], 16, 32); err == nil {
						b.WriteRune(rune(cp))
						i += 5
						continue
					}
				}
			}
		}
		b.WriteByte(c)
	}
	return b.String()
}

// jsonFlatten renders a value as JSON and reads it back as plain
// map/slice/float64/string/bool/nil. A value that will not marshal is
// returned as it is: the comparison then fails and prints it, which says
// more than a panic here would.
func jsonFlatten(v any) any {
	raw, err := json.Marshal(v)
	if err != nil {
		return v
	}
	var out any
	if err := json.Unmarshal(raw, &out); err != nil {
		return v
	}
	return out
}

// Compile-time assertion that specEntry stringifies meaningfully in
// error messages (keeps `fmt` import stable if trimmed elsewhere).
var _ = fmt.Sprintf
