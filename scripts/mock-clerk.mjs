// ローカル検証用の偽の Clerk。本物の Clerk には接続しない。
//   - GET /.well-known/jwks.json   公開鍵
//   - GET /v1/users/:id            ユーザー（下の USERS）
//   - GET /token/:id               そのユーザーのセッショントークン（RS256）を発行
// 使い方: node scripts/mock-clerk.mjs [port]
// .dev.vars に CLERK_ISSUER=http://localhost:<port> と CLERK_API_URL=http://localhost:<port>/v1 を書く。
import { createServer } from "node:http";
import { generateKeyPairSync, createSign } from "node:crypto";

const port = Number(process.argv[2] ?? 9871);
const issuer = `http://localhost:${port}`;
const azp = process.env.MOCK_AZP ?? "http://localhost:5173";
const { publicKey, privateKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
const jwk = { ...publicKey.export({ format: "jwk" }), kid: "mock-key", alg: "RS256", use: "sig" };

const email = (addr) => [{ id: "idn_1", email_address: addr, verification: { status: "verified" } }];
const google = (addr) => [{ provider: "oauth_google", email_address: addr, verification: { status: "verified" } }];
const USERS = {
  user_admin: { first_name: "Admin", email_addresses: email("admin@example.test"), external_accounts: google("admin@example.test"), public_metadata: {} },
  user_member: { first_name: "Member", email_addresses: email("member@example.test"), external_accounts: google("member@example.test"), public_metadata: { stock_insight: true } },
  user_outsider: { first_name: "Outsider", email_addresses: email("outsider@example.test"), external_accounts: google("outsider@example.test"), public_metadata: {} },
  user_password: { first_name: "Password", email_addresses: email("pw@example.test"), external_accounts: [], public_metadata: { stock_insight: true } },
};

const b64 = (o) => Buffer.from(typeof o === "string" ? o : JSON.stringify(o)).toString("base64url");
function token(sub, overrides = {}) {
  const now = Math.floor(Date.now() / 1000);
  const head = b64({ alg: "RS256", kid: "mock-key", typ: "JWT" });
  const body = b64({ sub, iss: issuer, azp, iat: now, nbf: now - 5, exp: now + 60, ...overrides });
  const sig = createSign("RSA-SHA256").update(`${head}.${body}`).sign(privateKey).toString("base64url");
  return `${head}.${body}.${sig}`;
}

createServer((req, res) => {
  const url = new URL(req.url, issuer);
  const send = (status, body) => {
    // 画面の開発サーバー（別オリジン）からトークンを取れるようにする
    res.writeHead(status, { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" });
    res.end(typeof body === "string" ? body : JSON.stringify(body));
  };
  if (url.pathname === "/.well-known/jwks.json") return send(200, { keys: [jwk] });
  let m = url.pathname.match(/^\/v1\/users\/(\w+)$/);
  if (m) {
    if (!req.headers.authorization?.startsWith("Bearer ")) return send(401, { errors: [] });
    const u = USERS[m[1]];
    return u ? send(200, { id: m[1], primary_email_address_id: "idn_1", ...u }) : send(404, { errors: [] });
  }
  m = url.pathname.match(/^\/token\/(\w+)$/);
  if (m) {
    const extra = url.searchParams.get("azp") ? { azp: url.searchParams.get("azp") } : {};
    if (url.searchParams.get("expired")) Object.assign(extra, { exp: Math.floor(Date.now() / 1000) - 3600 });
    return send(200, token(m[1], extra));
  }
  send(404, { error: "not found" });
}).listen(port, () => console.log(`mock clerk on ${issuer}`));
