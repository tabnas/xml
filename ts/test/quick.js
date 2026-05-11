const { Jsonic } = require('jsonic')
const { Xml } = require('../dist/xml')

const xml = Jsonic.make().use(Xml)

console.log(
  JSON.stringify(
    xml('<root><a>hello</a><b/></root>'),
    null,
    2,
  ),
)
