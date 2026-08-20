// Copyright (c) 2021-2026 Richard Rodger, MIT License

package tabnasxml

// Error COLUMNS after a non-ASCII character.
//
// This plugin brings its own matchers, and a plugin that does owns the
// arithmetic the engine's matchers do for it: SI advances in BYTES and CI
// in RUNES. `advance` added `to - from` to both, so a 2-byte `é` charged
// two columns, a 3-byte `€` three and an astral character four — every
// diagnostic after a non-ASCII character reported a column past where the
// problem was.
//
// The TypeScript half writes `pnt.cI += end - sI` over UTF-16 indices,
// which DOES count characters. The two lines look like the same line,
// which is why this survived: a transliteration is not a port when the
// two languages index strings differently.
//
// ts/test/xml.test.ts 'error columns count characters, not bytes' asserts
// the same four inputs. The astral row is the only one where the answers
// differ, and that is the recorded engine divergence — TypeScript counts
// UTF-16 units (an astral character is 2), Go counts runes (1). See
// parser/DIVERGENCE.md, "Column positions for astral characters".
//
// Found by the fleet parity probe; the same defect was repaired in
// tabnas/toml the same day.

import (
	"encoding/json"
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
)

func TestErrorColumnsCountRunesNotBytes(t *testing.T) {
	for _, c := range []struct {
		label string
		src   string
		col   int
		ts    int // what the TypeScript half asserts, for the reader
	}{
		// Control: pure ASCII, where bytes and runes coincide. Without
		// it, "columns count characters" is also satisfied by never
		// counting.
		{"ascii", "<a>xx</a><", 10, 10},

		// 2 and 3 bytes, 1 rune, 1 UTF-16 unit: both ports agree.
		{"latin1", "<a>é</a><", 9, 9},
		{"bmp", "<a>€</a><", 9, 9},

		// 4 bytes, 1 rune, TWO UTF-16 units: the recorded divergence, and
		// the only row where the two halves differ.
		{"astral", "<a>\U0001F600</a><", 9, 10},
	} {
		j := jsonic.Make(jsonic.Options{})
		if err := j.Use(Xml, map[string]any{}); err != nil {
			t.Fatalf("%s: use: %v", c.label, err)
		}
		_, err := j.Parse(c.src)
		if err == nil {
			t.Errorf("%s: %q parsed, expected a diagnostic", c.label, c.src)
			continue
		}
		b, mErr := json.Marshal(err)
		if mErr != nil {
			t.Fatalf("%s: marshal: %v", c.label, mErr)
		}
		var o struct {
			Col   int            `json:"col"`
			Token map[string]any `json:"token"`
		}
		if uErr := json.Unmarshal(b, &o); uErr != nil {
			t.Fatalf("%s: unmarshal: %v", c.label, uErr)
		}
		if o.Col != c.col {
			t.Errorf("%s: %q col = %d, want %d (TypeScript says %d). A column "+
				"ahead of the want by the character's extra BYTES means "+
				"`advance` is counting bytes again.",
				c.label, c.src, o.Col, c.col, c.ts)
		}
	}
}
