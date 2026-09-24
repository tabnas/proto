// protoc's own .proto parser, driven the way parser_unittest.cc's ParserTest
// drives it: an io::Tokenizer over the bytes, compiler::Parser with no
// required syntax identifier, and errors collected as "L:C: msg\n" (0-based,
// as upstream's MockErrorCollector writes them). No descriptor pool is
// built, so options stay uninterpreted and names stay unresolved: this is
// the output the corpus goldens describe.
//
// For each file named on the command line it prints one JSON line:
//   {"file":..., "ok":bool, "end":bool, "errors":"...", "warnings":"...",
//    "descriptor":{...}}
// with source_code_info cleared, as ExpectParsesTo clears it.
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>

#include "absl/strings/substitute.h"
#include "google/protobuf/compiler/parser.h"
#include "google/protobuf/descriptor.pb.h"
#include "google/protobuf/io/tokenizer.h"
#include "google/protobuf/io/zero_copy_stream_impl_lite.h"
#include "google/protobuf/struct.pb.h"
#include "google/protobuf/util/json_util.h"

namespace gp = google::protobuf;

class Collector : public gp::io::ErrorCollector {
 public:
  std::string warning_;
  std::string text_;
  void RecordWarning(int line, int column, absl::string_view message) override {
    absl::SubstituteAndAppend(&warning_, "$0:$1: $2\n", line, column, message);
  }
  void RecordError(int line, int column, absl::string_view message) override {
    absl::SubstituteAndAppend(&text_, "$0:$1: $2\n", line, column, message);
  }
};

// A JSON string literal, through the library's own JSON printer.
static std::string Quote(const std::string& s) {
  gp::Value v;
  v.set_string_value(s);
  std::string out;
  (void)gp::util::MessageToJsonString(v, &out);
  return out;
}

int main(int argc, char** argv) {
  for (int i = 1; i < argc; i++) {
    std::ifstream in(argv[i], std::ios::binary);
    std::stringstream ss;
    ss << in.rdbuf();
    const std::string text = ss.str();
    gp::io::ArrayInputStream raw(text.data(), static_cast<int>(text.size()));
    Collector errors;
    gp::io::Tokenizer input(&raw, &errors);
    gp::compiler::Parser parser;
    parser.RecordErrorsTo(&errors);
    parser.SetRequireSyntaxIdentifier(false);
    gp::FileDescriptorProto file;
    const bool ok = parser.Parse(&input, &file);
    const bool end = gp::io::Tokenizer::TYPE_END == input.current().type;
    file.clear_source_code_info();
    std::string json;
    (void)gp::util::MessageToJsonString(file, &json);
    std::cout << "{\"file\":" << Quote(argv[i]) << ",\"ok\":" << (ok ? "true" : "false")
              << ",\"end\":" << (end ? "true" : "false")
              << ",\"errors\":" << Quote(errors.text_)
              << ",\"warnings\":" << Quote(errors.warning_)
              << ",\"descriptor\":" << json << "}\n";
  }
  return 0;
}
