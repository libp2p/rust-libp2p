// This is free and unencumbered software released into the public domain.

// Anyone is free to copy, modify, publish, use, compile, sell, or
// distribute this software, either in source code form or as a compiled
// binary, for any purpose, commercial or non-commercial, and by any
// means.

// In jurisdictions that recognize copyright laws, the author or authors
// of this software dedicate any and all copyright interest in the
// software to the public domain. We make this dedication for the benefit
// of the public at large and to the detriment of our heirs and
// successors. We intend this dedication to be an overt act of
// relinquishment in perpetuity of all present and future rights to this
// software under copyright law.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY CLAIM, DAMAGES OR
// OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
// ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
// OTHER DEALINGS IN THE SOFTWARE.

// For more information, please refer to <http://unlicense.org/>
pub struct MessageCache {
    msgs: Map<MsgId>
}

// func NewMessageCache(gossip, history int) *MessageCache {
// 	return &MessageCache{
// 		msgs:    make(map[string]*pb.Message),
// 		history: make([][]CacheEntry, history),
// 		gossip:  gossip,
// 	}
// }

// type MessageCache struct {
// 	msgs    map[string]*pb.Message
// 	history [][]CacheEntry
// 	gossip  int
// }

// type CacheEntry struct {
// 	mid    string
// 	topics []string
// }

// func (mc *MessageCache) Put(msg *pb.Message) {
// 	mid := msgID(msg)
// 	mc.msgs[mid] = msg
// 	mc.history[0] = append(mc.history[0], CacheEntry{mid: mid, topics: msg.GetTopicIDs()})
// }

// func (mc *MessageCache) Get(mid string) (*pb.Message, bool) {
// 	m, ok := mc.msgs[mid]
// 	return m, ok
// }

// func (mc *MessageCache) GetGossipIDs(topic string) []string {
// 	var mids []string
// 	for _, entries := range mc.history[:mc.gossip] {
// 		for _, entry := range entries {
// 			for _, t := range entry.topics {
// 				if t == topic {
// 					mids = append(mids, entry.mid)
// 					break
// 				}
// 			}
// 		}
// 	}
// 	return mids
// }

// func (mc *MessageCache) Shift() {
// 	last := mc.history[len(mc.history)-1]
// 	for _, entry := range last {
// 		delete(mc.msgs, entry.mid)
// 	}
// 	for i := len(mc.history) - 2; i >= 0; i-- {
// 		mc.history[i+1] = mc.history[i]
// 	}
// 	mc.history[0] = nil
// }