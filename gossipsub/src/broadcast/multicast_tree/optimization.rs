// ### Multicast Tree Optimization (TODO)

// The multicast tree is constructed lazily, following the path of the
// first published message from some source. Therefore, the tree may not
// directly take advantage of new paths that may appear in the overlay as
// a result of new nodes/links. The overlay may also be suboptimal for
// all but the first source.

// To overcome these limitations and adapt the overlay to multiple
// sources, the authors in [1] propose an optimization: every time a
// message is received, it is checked against the missing list and the
// hopcount of messages in the list. If the eager transmission hopcount
// exceeds the hopcount of the lazy transmission, then the tree is
// candidate for optimization. If the tree were optimal, then the
// hopcount for messages received by eager push should be less than or
// equal to the hopcount of messages propagated by lazy push. Thus the
// eager link can be replaced by the lazy link and result to a shorter
// tree.

// To promote stability in the tree, the authors in [1] suggest that this
// optimization be performed only if the difference in hopcount is greater
// than a threshold value. This value is a design parameter that affects
// the overall stability of the tree: the lower the value, the more
// easier the protocol will try to optimize the tree by exchanging
// links. But if the threshold value is too low, it may result in
// fluttering with multiple active sources. Thus, the value should be
// higher and closer to the diameter of the tree to avoid constant
// changes.