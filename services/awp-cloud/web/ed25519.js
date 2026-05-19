// ed25519 signature verification (verify-only) — pure JavaScript, no
// external dependencies. Intended for the AWP audit viewer, where an
// auditor opens index.html locally and re-checks signed receipts in their
// own browser without trusting the page or any CDN.
//
// Reference: RFC 8032 §5.1.7 ("Verify"). This implementation follows the
// spec literally; it favours legibility and auditability over speed. It
// is verify-only — there are no signing primitives here.
//
// Public surface:
//   ed25519.verify(signatureHex, messageBytes, publicKeyHex) -> Promise<boolean>
//   ed25519.utils.hexToBytes(hex) -> Uint8Array
//   ed25519.utils.bytesToHex(bytes) -> string
//
// SHA-512 is taken from the Web Crypto API (window.crypto.subtle.digest).
// Browsers expose subtle only in secure contexts (https / localhost / file://
// in most browsers). file:// works on Firefox and Chromium when the page is
// the active origin.

(function (root, factory) {
    "use strict";
    if (typeof module === "object" && module.exports) {
        module.exports = factory();
    } else {
        root.ed25519 = factory();
    }
})(typeof self !== "undefined" ? self : this, function () {
    "use strict";

    // ---- field arithmetic over GF(2^255 - 19) -------------------------------

    const P = (1n << 255n) - 19n;
    // Curve order L = 2^252 + 27742317777372353535851937790883648493
    const L = (1n << 252n) + 27742317777372353535851937790883648493n;
    const D = -121665n * modInverse(121666n, P) % P;
    const D_POS = ((D % P) + P) % P;
    // Square root of -1 mod p, used in point decompression.
    const SQRT_M1 = modPow(2n, (P - 1n) / 4n, P);

    function mod(a, m) {
        const r = a % m;
        return r >= 0n ? r : r + m;
    }

    function modPow(base, exp, m) {
        let result = 1n;
        base = mod(base, m);
        while (exp > 0n) {
            if (exp & 1n) result = mod(result * base, m);
            exp >>= 1n;
            base = mod(base * base, m);
        }
        return result;
    }

    function modInverse(a, m) {
        // Fermat's little theorem: a^(p-2) mod p, valid since p is prime.
        return modPow(a, m - 2n, m);
    }

    // ---- extended (twisted Edwards) point arithmetic -----------------------
    //
    // Curve equation: -x^2 + y^2 = 1 + d*x^2*y^2  (twisted Edwards)
    // Extended coordinates (X, Y, Z, T) with x=X/Z, y=Y/Z, T=XY/Z.
    // Formulas: RFC 8032 §5.1.4 + standard add/double for extended coords.

    // Base point B (RFC 8032 §5.1).
    const BY = 4n * modInverse(5n, P) % P;
    const BX = recoverX(BY, 0n);

    function recoverX(y, signBit) {
        // Solve x^2 = (y^2 - 1) / (d*y^2 + 1) mod p.
        const y2 = mod(y * y, P);
        const u = mod(y2 - 1n, P);
        const v = mod(D_POS * y2 + 1n, P);
        // Compute x = uv^3 * (uv^7)^((p-5)/8)  per RFC 8032 §5.1.3.
        const v3 = mod(v * v % P * v, P);
        const v7 = mod(v3 * v3 % P * v, P);
        let x = mod(u * v3 % P * modPow(u * v7, (P - 5n) / 8n, P), P);
        // Verify and correct.
        const vx2 = mod(v * x % P * x, P);
        if (vx2 === u) {
            // ok
        } else if (vx2 === mod(-u, P)) {
            x = mod(x * SQRT_M1, P);
        } else {
            return null; // not on curve
        }
        if (x === 0n && signBit !== 0n) return null;
        if ((x & 1n) !== signBit) x = mod(-x, P);
        return x;
    }

    function pointDouble(P1) {
        const [X1, Y1, Z1] = P1;
        const A = mod(X1 * X1, P);
        const B = mod(Y1 * Y1, P);
        const C = mod(2n * Z1 * Z1, P);
        const H = mod(A + B, P);
        const E = mod(H - mod((X1 + Y1) * (X1 + Y1), P), P);
        const G = mod(A - B, P);
        const F = mod(C + G, P);
        return [
            mod(E * F, P),
            mod(G * H, P),
            mod(F * G, P),
            mod(E * H, P),
        ];
    }

    function pointAdd(P1, P2) {
        const [X1, Y1, Z1, T1] = P1;
        const [X2, Y2, Z2, T2] = P2;
        const A = mod((Y1 - X1) * (Y2 - X2), P);
        const B = mod((Y1 + X1) * (Y2 + X2), P);
        const C = mod(T1 * 2n * D_POS * T2, P);
        const Dt = mod(Z1 * 2n * Z2, P);
        const E = mod(B - A, P);
        const F = mod(Dt - C, P);
        const G = mod(Dt + C, P);
        const H = mod(B + A, P);
        return [
            mod(E * F, P),
            mod(G * H, P),
            mod(F * G, P),
            mod(E * H, P),
        ];
    }

    function pointEqual(P1, P2) {
        const [X1, Y1, Z1] = P1;
        const [X2, Y2, Z2] = P2;
        if (mod(X1 * Z2 - X2 * Z1, P) !== 0n) return false;
        if (mod(Y1 * Z2 - Y2 * Z1, P) !== 0n) return false;
        return true;
    }

    function scalarMul(scalar, point) {
        // Double-and-add. Constant-timeness is irrelevant for a verifier
        // that handles only public data.
        let result = [0n, 1n, 1n, 0n]; // identity (neutral element)
        let addend = point;
        let s = scalar;
        while (s > 0n) {
            if (s & 1n) result = pointAdd(result, addend);
            addend = pointDouble(addend);
            s >>= 1n;
        }
        return result;
    }

    function pointDecode(bytes32) {
        // RFC 8032 §5.1.3 step 2.
        if (bytes32.length !== 32) return null;
        const yBytes = new Uint8Array(bytes32);
        const signBit = BigInt((yBytes[31] >> 7) & 1);
        yBytes[31] &= 0x7f;
        const y = bytesToBigIntLE(yBytes);
        if (y >= P) return null;
        const x = recoverX(y, signBit);
        if (x === null) return null;
        return [x, y, 1n, mod(x * y, P)];
    }

    // ---- byte / bigint helpers ---------------------------------------------

    function bytesToBigIntLE(bytes) {
        let n = 0n;
        for (let i = bytes.length - 1; i >= 0; i--) {
            n = (n << 8n) | BigInt(bytes[i]);
        }
        return n;
    }

    function hexToBytes(hex) {
        if (typeof hex !== "string") throw new Error("hex must be a string");
        if (hex.length % 2 !== 0) throw new Error("hex length must be even");
        const out = new Uint8Array(hex.length / 2);
        for (let i = 0; i < out.length; i++) {
            const byte = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
            if (Number.isNaN(byte)) throw new Error("invalid hex");
            out[i] = byte;
        }
        return out;
    }

    function bytesToHex(bytes) {
        const hex = [];
        for (let i = 0; i < bytes.length; i++) {
            hex.push(bytes[i].toString(16).padStart(2, "0"));
        }
        return hex.join("");
    }

    function concatBytes(...arrays) {
        let total = 0;
        for (const a of arrays) total += a.length;
        const out = new Uint8Array(total);
        let off = 0;
        for (const a of arrays) {
            out.set(a, off);
            off += a.length;
        }
        return out;
    }

    async function sha512(bytes) {
        const buf = await crypto.subtle.digest("SHA-512", bytes);
        return new Uint8Array(buf);
    }

    // ---- public verify ------------------------------------------------------

    async function verify(signature, message, publicKey) {
        // Accept hex strings or Uint8Arrays for sig/pub.
        const sig =
            typeof signature === "string" ? hexToBytes(signature) : signature;
        const pub =
            typeof publicKey === "string" ? hexToBytes(publicKey) : publicKey;
        const msg =
            typeof message === "string"
                ? new TextEncoder().encode(message)
                : message;
        if (!(sig instanceof Uint8Array) || sig.length !== 64) return false;
        if (!(pub instanceof Uint8Array) || pub.length !== 32) return false;
        if (!(msg instanceof Uint8Array)) return false;

        const A = pointDecode(pub);
        if (A === null) return false;

        const Rbytes = sig.slice(0, 32);
        const sBytes = sig.slice(32, 64);
        const s = bytesToBigIntLE(sBytes);
        if (s >= L) return false;

        // k = SHA-512(R || A || M) mod L
        const k =
            mod(
                bytesToBigIntLE(await sha512(concatBytes(Rbytes, pub, msg))),
                L,
            );

        // Compute [s]B and R + [k]A; verify [8][s]B == [8](R + [k]A).
        // Multiplying both sides by 8 clears the cofactor and matches the
        // RFC 8032 §5.1.7 verification equation.
        const sB = scalarMul(s, [BX, BY, 1n, mod(BX * BY, P)]);
        const R = pointDecode(Rbytes);
        if (R === null) return false;
        const kA = scalarMul(k, A);
        const rhs = pointAdd(R, kA);

        const sB8 = pointDouble(pointDouble(pointDouble(sB)));
        const rhs8 = pointDouble(pointDouble(pointDouble(rhs)));
        return pointEqual(sB8, rhs8);
    }

    return {
        verify: verify,
        utils: {
            hexToBytes: hexToBytes,
            bytesToHex: bytesToHex,
        },
    };
});
