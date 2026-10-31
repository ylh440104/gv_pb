#include <cstdio>
#include <cstring>
#include <string>

#include "main.pb.h"
#include "session.pb.h"
#include "input_event.pb.h"
#include "rpc_message.pb.h"
#include "defines.pb.h"
#include <google/protobuf/stubs/common.h>

static int g_failures = 0;

static void expect(bool ok, const char* what) {
  if (!ok) {
    std::printf("  FAIL %s\n", what);
    ++g_failures;
  }
}

static void append_varint(std::string& out, uint64_t value) {
  while (value >= 0x80) {
    out.push_back(static_cast<char>((value & 0x7f) | 0x80));
    value >>= 7;
  }
  out.push_back(static_cast<char>(value));
}

static void append_tag(std::string& out, int field, int wire) {
  append_varint(out, static_cast<uint64_t>((field << 3) | wire));
}

static void append_float(std::string& out, int field, float value) {
  append_tag(out, field, 5);
  char buffer[4];
  std::memcpy(buffer, &value, 4);
  out.append(buffer, 4);
}

static void test_roundtrip() {
  std::printf("[roundtrip]\n");
  int before = g_failures;

  gv_pb::Message outbound;
  outbound.set_seq(1001);
  outbound.set_timestamp(1699999999);

  gv_pb::RpcRequest* rpc = outbound.mutable_rpc_request();
  rpc->mutable_request_header()->set_request_id(42);

  gv_pb::CaptureSettingRequest* capture = rpc->mutable_capture_setting();
  capture->set_fps(gv_pb::FPS_60);
  capture->set_frame_quality(gv_pb::VideoQuality_HD);
  capture->set_codec_type(gv_pb::CodecType_H265);
  capture->set_resolution_width(2560);
  capture->set_resolution_height(1440);
  capture->set_chorma_format(gv_pb::ChromaFormat_420);
  capture->set_cursor_capture(true);
  capture->set_max_custom_bitrate(20000000);

  std::string wire;
  expect(outbound.SerializeToString(&wire), "serialize");

  gv_pb::Message inbound;
  expect(inbound.ParseFromString(wire), "parse");
  expect(inbound.seq() == 1001, "seq");
  expect(inbound.timestamp() == 1699999999LL, "timestamp");
  expect(inbound.rpc_request().request_header().request_id() == 42, "request_id");

  const gv_pb::CaptureSettingRequest& back = inbound.rpc_request().capture_setting();
  expect(back.fps() == gv_pb::FPS_60, "fps");
  expect(back.frame_quality() == gv_pb::VideoQuality_HD, "frame_quality");
  expect(back.codec_type() == gv_pb::CodecType_H265, "codec_type");
  expect(back.resolution_width() == 2560, "width");
  expect(back.resolution_height() == 1440, "height");
  expect(back.chorma_format() == gv_pb::ChromaFormat_420, "chroma");
  expect(back.cursor_capture(), "cursor_capture");
  expect(back.max_custom_bitrate() == 20000000, "bitrate");

  std::printf("  %s (%zu bytes)\n", g_failures == before ? "ok" : "failed", wire.size());
}

static void test_external_bytes() {
  std::printf("[external]\n");
  int before = g_failures;

  std::string point;
  append_float(point, 2, 1.5f);
  append_float(point, 3, 2.5f);

  std::string touch;
  append_tag(touch, 1, 0);
  append_varint(touch, 2);
  append_tag(touch, 2, 2);
  append_varint(touch, point.size());
  touch += point;

  std::string event;
  append_tag(event, 1, 2);
  append_varint(event, touch.size());
  event += touch;

  std::string wire;
  append_tag(wire, 1, 0);
  append_varint(wire, 7);
  append_tag(wire, 20, 2);
  append_varint(wire, event.size());
  wire += event;

  gv_pb::Message parsed;
  expect(parsed.ParseFromString(wire), "parse external");
  expect(parsed.seq() == 7, "external seq");
  expect(parsed.has_input_event(), "input_event present");
  expect(parsed.input_event().touch_event().touch_points_size() == 1, "point count");

  if (parsed.input_event().touch_event().touch_points_size() == 1) {
    const gv_pb::TouchEventPoint& p = parsed.input_event().touch_event().touch_points(0);
    expect(p.x() > 1.49f && p.x() < 1.51f, "point x");
    expect(p.y() > 2.49f && p.y() < 2.51f, "point y");
  }
  std::printf("  %s\n", g_failures == before ? "ok" : "failed");
}

static void test_envelope() {
  std::printf("[envelope]\n");
  int before = g_failures;

  const google::protobuf::Descriptor* d = gv_pb::Message::descriptor();
  expect(d->field_count() == 28, "arm count");

  struct Pair {
    const char* name;
    int number;
  };
  const Pair pairs[] = {
      {"seq", 1},
      {"timestamp", 2},
      {"codec_negotiation", 18},
      {"input_event", 20},
      {"rpc_request", 21},
      {"rpc_response", 22},
      {"terminal_session_changed", 28},
  };
  for (const Pair& p : pairs) {
    const google::protobuf::FieldDescriptor* f = d->FindFieldByName(p.name);
    expect(f != nullptr && f->number() == p.number, p.name);
  }

  const google::protobuf::EnumDescriptor* action =
      gv_pb::SimpleAction::ActionType_descriptor();
  expect(action != nullptr, "action enum");
  if (action != nullptr) {
    expect(action->FindValueByNumber(0)->name() == "ACTION_TYPE_ECHO_REQUEST", "echo request");
    expect(action->FindValueByNumber(1)->name() == "ACTION_TYPE_ECHO_RESPONSE", "echo response");
    expect(action->FindValueByNumber(4)->name() == "ACTION_TYPE_CAPTURE_DESKTOP", "capture desktop");
    expect(action->value_count() == 28, "action count");
  }
  std::printf("  %s\n", g_failures == before ? "ok" : "failed");
}

int main() {
  GOOGLE_PROTOBUF_VERIFY_VERSION;
  std::printf("gv_pb wire verification, protobuf %s\n\n",
              google::protobuf::internal::VersionString(GOOGLE_PROTOBUF_VERSION).c_str());

  test_roundtrip();
  test_external_bytes();
  test_envelope();

  std::printf("\n%s (%d failures)\n", g_failures == 0 ? "passed" : "failed", g_failures);
  return g_failures == 0 ? 0 : 1;
}