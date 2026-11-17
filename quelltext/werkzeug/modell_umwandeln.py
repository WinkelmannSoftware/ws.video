# Wandelt das MediaPipe-Selfie-Modell (TFLite, float16) in ONNX (NCHW, float32) um.
import sys, struct, numpy as np, tflite, onnx
from onnx import helper as h, numpy_helper as nh, TensorProto as TP
src, dst = sys.argv[1], sys.argv[2]
buf = open(src, 'rb').read()
m = tflite.Model.GetRootAsModel(buf, 0); g = m.Subgraphs(0)
names = {v: k for k, v in vars(tflite.BuiltinOperator).items() if isinstance(v, int)}
def const(i):
    t = g.Tensors(i); b = m.Buffers(t.Buffer()).DataAsNumpy()
    if isinstance(b, int): return None
    dt = {0: np.float32, 1: np.float16, 2: np.int32}[t.Type()]
    return np.frombuffer(b.tobytes(), dtype=dt).reshape(t.ShapeAsNumpy())
consts = {}; nodes = []; inits = []
def T(i): return f"t{i}"
def add_init(name, arr): inits.append(nh.from_array(np.ascontiguousarray(arr), name)); return name
def cval(i):
    if i in consts: return consts[i]
    return const(i)
for k in range(g.OperatorsLength()):
    op = g.Operators(k); oc = m.OperatorCodes(op.OpcodeIndex()); n = names[oc.BuiltinCode()]
    ins = [int(x) for x in op.InputsAsNumpy()]; outs = [int(x) for x in op.OutputsAsNumpy()]
    o = T(outs[0])
    def opts(cls):
        x = cls(); x.Init(op.BuiltinOptions().Bytes, op.BuiltinOptions().Pos); return x
    def act(code, name):
        assert code == 0, code
    if n == 'DEQUANTIZE':
        consts[outs[0]] = const(ins[0]).astype(np.float32)
    elif n in ('CONV_2D', 'DEPTHWISE_CONV_2D'):
        w = cval(ins[1]).astype(np.float32); b = cval(ins[2]).astype(np.float32)
        if n == 'CONV_2D':
            x = opts(tflite.Conv2DOptions); w = w.transpose(0, 3, 1, 2); group = 1
        else:
            x = opts(tflite.DepthwiseConv2DOptions); w = w.transpose(3, 0, 1, 2); group = w.shape[0]
        act(x.FusedActivationFunction(), n)
        kw = dict(strides=[x.StrideH(), x.StrideW()], group=group, kernel_shape=list(w.shape[2:]))
        if x.Padding() == 0:
            ish = g.Tensors(ins[0]).ShapeAsNumpy(); pads = []
            for d in (0, 1):
                size = int(ish[1 + d]); st = kw['strides'][d]; kk = w.shape[2 + d]
                total = max((-(-size // st) - 1) * st + kk - size, 0); pads.append((total // 2, total - total // 2))
            kw['pads'] = [pads[0][0], pads[1][0], pads[0][1], pads[1][1]]
        nodes.append(h.make_node('Conv', [T(ins[0]), add_init(f"w{k}", w), add_init(f"b{k}", b)], [o], **kw))
    elif n == 'CUSTOM':
        assert oc.CustomCode() == b'Convolution2DTransposeBias'
        pad, sw, sh = struct.unpack('<iii', bytes(op.CustomOptionsAsNumpy())[:12])
        w = cval(ins[1]).astype(np.float32).transpose(3, 0, 1, 2); b = cval(ins[2]).astype(np.float32)
        assert pad == 1 or (w.shape[2] == sh)
        nodes.append(h.make_node('ConvTranspose', [T(ins[0]), add_init(f"w{k}", w), add_init(f"b{k}", b)], [o], strides=[sh, sw], kernel_shape=list(w.shape[2:])))
    elif n == 'AVERAGE_POOL_2D':
        x = opts(tflite.Pool2DOptions); shp = g.Tensors(ins[0]).ShapeAsNumpy()
        assert x.FilterHeight() == shp[1] and x.FilterWidth() == shp[2]
        nodes.append(h.make_node('GlobalAveragePool', [T(ins[0])], [o]))
    elif n in ('ADD', 'MUL'):
        a = []
        for i in ins:
            c = cval(i)
            if c is not None:
                c = c.astype(np.float32)
                if c.ndim == 4: c = c.transpose(0, 3, 1, 2)
                elif c.ndim == 1 and c.size > 1: c = c.reshape(1, -1, 1, 1)
                a.append(add_init(f"c{k}_{i}", c))
            else: a.append(T(i))
        nodes.append(h.make_node('Add' if n == 'ADD' else 'Mul', a, [o]))
    elif n == 'RESIZE_BILINEAR':
        # 2x bilinear (half pixel centers) als Rand-Auffuellung + ConvTranspose + Zuschnitt, weil das ueberall gleich rechnet
        x = opts(tflite.ResizeBilinearOptions); size = cval(ins[1]); ish = g.Tensors(ins[0]).ShapeAsNumpy()
        assert x.HalfPixelCenters() and size[0] == 2 * ish[1] and size[1] == 2 * ish[2]
        C = int(ish[3]); k1 = np.array([0.25, 0.75, 0.75, 0.25], dtype=np.float32)
        w = np.tile(np.outer(k1, k1)[None, None], (C, 1, 1, 1)).astype(np.float32)
        pads = add_init(f"p{k}", np.array([0, 0, 1, 1, 0, 0, 1, 1], dtype=np.int64))
        nodes.append(h.make_node('Pad', [T(ins[0]), pads], [o + '_p'], mode='edge'))
        nodes.append(h.make_node('ConvTranspose', [o + '_p', add_init(f"w{k}", w)], [o + '_u'], strides=[2, 2], kernel_shape=[4, 4], group=C))
        st = add_init(f"ss{k}", np.array([3, 3], dtype=np.int64)); en = add_init(f"se{k}", np.array([3 + int(size[0]), 3 + int(size[1])], dtype=np.int64)); ax = add_init(f"sa{k}", np.array([2, 3], dtype=np.int64))
        nodes.append(h.make_node('Slice', [o + '_u', st, en, ax], [o]))
    elif n == 'HARD_SWISH': nodes.append(h.make_node('HardSwish', [T(ins[0])], [o]))
    elif n == 'RELU': nodes.append(h.make_node('Relu', [T(ins[0])], [o]))
    elif n == 'LOGISTIC': nodes.append(h.make_node('Sigmoid', [T(ins[0])], [o]))
    else: raise SystemExit('unbekannt: ' + n)
ii = int(g.Inputs(0)); oi = int(g.Outputs(0))
ish = g.Tensors(ii).ShapeAsNumpy(); osh = g.Tensors(oi).ShapeAsNumpy()
inp = h.make_tensor_value_info(T(ii), TP.FLOAT, [1, 3, int(ish[1]), int(ish[2])])
out = h.make_tensor_value_info(T(oi), TP.FLOAT, [1, int(osh[3]), int(osh[1]), int(osh[2])])
model = h.make_model(h.make_graph(nodes, 'selfie', [inp], [out], inits), opset_imports=[h.make_opsetid('', 14)])
model.ir_version = 8
onnx.checker.check_model(model)
onnx.save(model, dst)
print('ok', dst, T(ii), T(oi), ish, osh)
