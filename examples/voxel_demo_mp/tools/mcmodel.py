"""Extract MC entity model geometry from compiled .class bytecode.

The client JAR is NOT obfuscated (readable net/minecraft/... names),
so the model builder methods call CubeListBuilder.texOffs(u,v)
.addBox(x,y,z,w,h,d)[.mirror()] and PartPose.offset[AndRotation](...)
with numeric constants pushed right before each call. We parse the
constant pool + the Code attribute of every method, walk the opcode
stream, and emit an ordered trace of (pushed-constants, method-name)
so the model can be transcribed faithfully.
"""
import sys, struct, zipfile

JAR = r'C:\Users\micha\AppData\Roaming\.minecraft\versions\26.1.2\26.1.2.jar'

# opcode -> operand byte length (for the ones we must skip past).
# We special-case push/ldc/invoke below; everything else just needs
# its operand length so we stay aligned.
OPLEN = {
    0x10:1,0x11:2,0x12:1,0x13:2,0x14:2,  # bipush sipush ldc ldc_w ldc2_w
    0x15:1,0x16:1,0x17:1,0x18:1,0x19:1,  # *load
    0x36:1,0x37:1,0x38:1,0x39:1,0x3a:1,  # *store
    0xa9:1,  # ret
    0x84:2,  # iinc
    0x99:2,0x9a:2,0x9b:2,0x9c:2,0x9d:2,0x9e:2,0x9f:2,0xa0:2,0xa1:2,0xa2:2,0xa3:2,0xa4:2,0xa5:2,0xa6:2,0xa7:2,0xa8:2,  # branches
    0xb2:2,0xb3:2,0xb4:2,0xb5:2,  # get/put field/static
    0xb6:2,0xb7:2,0xb8:2,  # invokevirtual/special/static
    0xb9:4,0xba:4,  # invokeinterface, invokedynamic
    0xbb:2,0xbc:1,0xbd:2,  # new, newarray, anewarray
    0xc0:2,0xc1:2,  # checkcast, instanceof
    0xc5:3,  # multianewarray
    0xc6:2,0xc7:2,  # ifnull ifnonnull
    0xc8:4,0xc9:4,  # goto_w jsr_w
}

def parse_class(data):
    off = 8  # skip magic(4)+minor(2)+major(2); cp_count is here
    cp_count, = struct.unpack_from('>H', data, off); off += 2
    cp = [None]*cp_count
    i = 1
    while i < cp_count:
        tag = data[off]; off += 1
        if tag == 1:  # Utf8
            ln, = struct.unpack_from('>H', data, off); off += 2
            cp[i] = ('Utf8', data[off:off+ln].decode('utf-8','replace')); off += ln
        elif tag == 3:  # Integer
            v, = struct.unpack_from('>i', data, off); off += 4; cp[i] = ('Int', v)
        elif tag == 4:  # Float
            v, = struct.unpack_from('>f', data, off); off += 4; cp[i] = ('Float', v)
        elif tag in (5,6):  # Long/Double (2 slots)
            if tag == 5:
                v, = struct.unpack_from('>q', data, off); cp[i]=('Long',v)
            else:
                v, = struct.unpack_from('>d', data, off); cp[i]=('Double',v)
            off += 8; i += 2; continue
        elif tag in (7,8,16,19,20):  # Class/String/MethodType/Module/Package -> 1 ref
            idx, = struct.unpack_from('>H', data, off); off += 2; cp[i] = (tag, idx)
        elif tag in (9,10,11,12,18,17):  # ref pairs
            a,b = struct.unpack_from('>HH', data, off); off += 4; cp[i] = (tag, a, b)
        elif tag == 15:  # MethodHandle
            off += 3; cp[i] = ('MH',)
        else:
            raise ValueError(f'unknown cp tag {tag} at {off}')
        i += 1
    return cp, off

def utf(cp, idx):
    e = cp[idx]
    return e[1] if e and e[0]=='Utf8' else f'#{idx}'

def methodref_name(cp, idx):
    e = cp[idx]
    if not e or e[0] not in (9,10,11): return f'#{idx}'
    nat = cp[e[2]]  # NameAndType
    return utf(cp, nat[1])

def const_val(cp, idx):
    e = cp[idx]
    if not e: return None
    if e[0] in ('Int','Float','Long','Double'): return e[1]
    if e[0] == 8:  # String
        return 'str:'+utf(cp, e[1])
    return None

def walk_code(cp, code):
    out = []
    p = 0
    n = len(code)
    while p < n:
        op = code[p]
        if op in (0x02,0x03,0x04,0x05,0x06,0x07,0x08):  # iconst_m1..5
            out.append(('push', op-3)); p += 1
        elif op in (0x0b,0x0c,0x0d):  # fconst_0..2
            out.append(('push', float(op-0x0b))); p += 1
        elif op == 0x0e or op == 0x0f:  # dconst_0/1
            out.append(('push', float(op-0x0e))); p += 1
        elif op == 0x10:  # bipush
            out.append(('push', struct.unpack_from('>b',code,p+1)[0])); p += 2
        elif op == 0x11:  # sipush
            out.append(('push', struct.unpack_from('>h',code,p+1)[0])); p += 3
        elif op in (0x12,):  # ldc
            out.append(('push', const_val(cp, code[p+1]))); p += 2
        elif op in (0x13,0x14):  # ldc_w / ldc2_w
            idx, = struct.unpack_from('>H',code,p+1)
            out.append(('push', const_val(cp, idx))); p += 3
        elif op in (0xb6,0xb7,0xb8):  # invoke virtual/special/static
            idx, = struct.unpack_from('>H',code,p+1)
            out.append(('call', methodref_name(cp, idx))); p += 3
        elif op == 0xb9:  # invokeinterface
            idx, = struct.unpack_from('>H',code,p+1)
            out.append(('call', methodref_name(cp, idx))); p += 5
        elif op == 0xc7 or op==0xc6:  # ifnull/nonnull
            p += 3
        elif op == 0xaa:  # tableswitch - align + parse
            pad = (4 - ((p+1) % 4)) % 4
            base = p+1+pad
            low, = struct.unpack_from('>i',code,base+4)
            high, = struct.unpack_from('>i',code,base+8)
            p = base + 12 + (high-low+1)*4
        elif op == 0xab:  # lookupswitch
            pad = (4 - ((p+1) % 4)) % 4
            base = p+1+pad
            npairs, = struct.unpack_from('>i',code,base+4)
            p = base + 8 + npairs*8
        elif op == 0xc4:  # wide
            wop = code[p+1]
            p += (6 if wop==0x84 else 4)
        else:
            p += 1 + OPLEN.get(op, 0)
    return out

def methods_with_addbox(cp, data, off):
    # access_flags u2 + this_class u2 + super_class u2 = 6 bytes
    off += 6
    icount, = struct.unpack_from('>H', data, off); off += 2 + 2*icount
    # fields
    fcount, = struct.unpack_from('>H', data, off); off += 2
    for _ in range(fcount):
        off += 6
        acount, = struct.unpack_from('>H', data, off); off += 2
        for _ in range(acount):
            _ni, alen = struct.unpack_from('>HI', data, off); off += 6 + alen
    # methods
    mcount, = struct.unpack_from('>H', data, off); off += 2
    results = []
    for _ in range(mcount):
        _acc, nidx, didx = struct.unpack_from('>HHH', data, off); off += 6
        mname = utf(cp, nidx)
        acount, = struct.unpack_from('>H', data, off); off += 2
        code_trace = None
        for _ in range(acount):
            ani, alen = struct.unpack_from('>HI', data, off); off += 6
            aname = utf(cp, ani)
            body = data[off:off+alen]
            if aname == 'Code':
                # max_stack u2, max_locals u2, code_length u4, code
                clen, = struct.unpack_from('>I', body, 4)
                code = body[8:8+clen]
                tr = walk_code(cp, code)
                if any(c==('call','addBox') for c in tr) or any(c==('call','texOffs') for c in tr):
                    code_trace = tr
            off += alen
        if code_trace:
            results.append((mname, code_trace))
    return results

def render_trace(mname, trace):
    print(f'=== method {mname} ===')
    buf = []
    line_parts = []
    for kind, val in trace:
        if kind == 'push':
            line_parts.append(repr(val))
        else:  # call
            if val in ('texOffs','addBox','offset','offsetAndRotation','rotation','mirror',
                       'addOrReplaceChild','create','setRotation','translateAndRotate'):
                args = ', '.join(line_parts)
                print(f'  .{val}({args})')
                line_parts = []
            elif val in ('addBox','texOffs'):
                pass
            else:
                # other call - flush accumulated pushes as context, drop
                if line_parts and val not in ('valueOf',):
                    line_parts = []
    _ = buf

if __name__ == '__main__':
    target = sys.argv[1] if len(sys.argv) > 1 else 'animal/chicken/AdultChickenModel'
    z = zipfile.ZipFile(JAR)
    path = f'net/minecraft/client/model/{target}.class'
    data = z.read(path)
    cp, off = parse_class(data)
    for mname, tr in methods_with_addbox(cp, data, off):
        render_trace(mname, tr)
