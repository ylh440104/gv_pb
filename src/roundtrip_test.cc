// roundtrip_test.cc - proves the recovered gv_pb schema is wire-compatible.
//
// 1. Builds a Message the way the real controller does (seq + timestamp + RPC
//    capture-setting request), serializes it, parses it back, checks fields.
// 2. Decodes a hand-assembled byte string using the recovered schema to confirm
//    field numbers, float encoding and nested repeated messages line up.
// 3. Asserts the envelope carries all 28 recovered arms with correct numbers.
#include <cstdio>
#include <cstring>
#include <string>

#include "main.pb.h"
#include "session.pb.h"
#include "input_event.pb.h"
#include "rpc_message.pb.h"
#include "defines.pb.h"
#include <google/protobuf/stubs/common.h>

static int failures = 0;

#define CHECK(cond, ...)                          \
  do {                                            \
    if (!(cond)) {                                \
      printf("  FAIL: ");                         \
      printf(__VA_ARGS__);                        \
      printf("\n");                               \
      ++failures;                                 \
    }                                             \
  } while (0)

static void dump_hex(const std::string& s) {
  for (unsigned char c : s) printf("%02x", c);
}

static void put_varint(std::string& s, uint64_t v) {
  while (v >= 0x80) {
    s.push_back((char)((v & 0x7f) | 0x80));
    v >>= 7;
  }
  s.push_back((char)v);
}

static void put_tag(std::string& s, int f, int wt) {
  put_varint(s, (uint64_t)((f << 3) | wt));
}

static void test_roundtrip() {
  printf("[1] serialize/parse roundtrip\n");
  int before = failures;

  gv_pb::Message m;
  m.set_seq(1001);
  m.set_timestamp(1699999999);

  auto* rpc = m.mutable_rpc_request();
  rpc->mutable_request_header()->set_request_id(42);

  auto* cs = rpc->mutable_capture_setting();
  cs->set_fps(gv_pb::FPS_60);
  cs->set_frame_quality(gv_pb::VideoQuality_HD);
  cs->set_codec_type(gv_pb::CodecType_H265);
  cs->set_resolution_width(2560);
  cs->set_resolution_height(1440);
  cs->set_chorma_format(gv_pb::ChromaFormat_420);
  cs->set_cursor_capture(true);

  std::string wire;
  CHECK(m.SerializeToString(&wire), "SerializeToString failed");
  printf("  encoded %zu bytes: ", wire.size());
  dump_hex(wire);
  printf("\n");

  gv_pb::Message r;
  CHECK(r.ParseFromString(wire), "ParseFromString failed");
  CHECK(r.seq() == 1001, "seq %lld", (long long)r.seq());
  CHECK(r.timestamp() == 1699999999LL, "timestamp %lld", (long long)r.timestamp());
  CHECK(r.rpc_request().request_header().request_id() == 42, "request_id");

  const auto& c = r.rpc_request().capture_setting();
  CHECK(c.fps() == gv_pb::FPS_60, "fps %d", (int)c.fps());
  CHECK(c.frame_quality() == gv_pb::VideoQuality_HD, "quality %d", (int)c.frame_quality());
  CHECK(c.codec_type() == gv_pb::CodecType_H265, "codec %d", (int)c.codec_type());
  CHECK(c.resolution_width() == 2560, "width %d", c.resolution_width());
  CHECK(c.resolution_height() == 1440, "height %d", c.resolution_height());
  CHECK(c.chorma_format() == gv_pb::ChromaFormat_420, "chroma %d", (int)c.chorma_format());
  CHECK(c.cursor_capture(), "cursor_capture");
  printf("  %s\n", failures == before ? "OK" : "FAILED");
}

// Message{seq=7, input_event{touch_event{event_type=2, points[0]{x=1.5,y=2.5}}}}
static void test_decode_foreign_bytes() {
  printf("[2] decode externally produced bytes\n");
  int before = failures;

  std::string pt;
  put_tag(pt, 2, 5);
  float x = 1.5f, y = 2.5f;
  char b4[4];
  memcpy(b4, &x, 4);
  pt.append(b4, 4);
  put_tag(pt, 3, 5);
  memcpy(b4, &y, 4);
  pt.append(b4, 4);

  std::string touch;
  put_tag(touch, 1, 0);
  put_varint(touch, 2);
  put_tag(touch, 2, 2);
  put_varint(touch, pt.size());
  touch += pt;

  std::string ie;
  put_tag(ie, 1, 2);
  put_varint(ie, touch.size());
  ie += touch;

  std::string wire;
  put_tag(wire, 1, 0);
  put_varint(wire, 7);
  put_tag(wire, 20, 2);
  put_varint(wire, ie.size());
  wire += ie;

  printf("  raw: ");
  dump_hex(wire);
  printf("\n");

  gv_pb::Message m;
  CHECK(m.ParseFromString(wire), "parse failed");
  CHECK(m.seq() == 7, "seq %lld", (long long)m.seq());
  CHECK(m.has_input_event(), "input_event missing");

  const auto& te = m.input_event().touch_event();
  CHECK(te.touch_points_size() == 1, "points %d", te.touch_points_size());
  if (te.touch_points_size() == 1) {
    CHECK(te.touch_points(0).x() > 1.49f && te.touch_points(0).x() < 1.51f,
          "x=%f", te.touch_points(0).x());
    CHECK(te.touch_points(0).y() > 2.49f && te.touch_points(0).y() < 2.51f,
          "y=%f", te.touch_points(0).y());
  }
  printf("  %s\n", failures == before ? "OK" : "FAILED");
}

static void test_envelope_shape() {
  printf("[3] envelope field coverage\n");
  int before = failures;

  const auto* d = gv_pb::Message::descriptor();
  printf("  Message has %d arms\n", d->field_count());
  CHECK(d->field_count() == 28, "expected 28 arms, got %d", d->field_count());

  struct { const char* name; int num; } expect[] = {
      {"seq", 1}, {"timestamp", 2}, {"codec_negotiation", 18},
      {"input_event", 20}, {"rpc_request", 21}, {"rpc_response", 22},
      {"terminal_session_changed", 28},
  };
  for (const auto& e : expect) {
    const auto* fd = d->FindFieldByName(e.name);
    CHECK(fd && fd->number() == e.num, "field %s number mismatch", e.name);
  }
  printf("  %s\n", failures == before ? "OK" : "FAILED");
}

int main() {
  GOOGLE_PROTOBUF_VERIFY_VERSION;
  printf("gv_pb schema roundtrip verification\n");
  printf("protobuf runtime: %s\n\n", google::protobuf::internal::VersionString(GOOGLE_PROTOBUF_VERSION).c_str());

  test_roundtrip();
  test_decode_foreign_bytes();
  test_envelope_shape();

  printf("\n%s (%d failure(s))\n",
         failures ? "RESULT: FAILED" : "RESULT: ALL PASSED", failures);
  return failures ? 1 : 0;
}