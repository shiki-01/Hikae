// iOS の署名に必要なものを、App Store Connect API で自動で作る（Mac で 1 回だけ実行する）。
// 作るもの: 配布用の証明書（Apple Distribution）、App ID、プロビジョニングプロファイル（App Store 配布用）。
// 出力: GitHub のシークレット 7 つ分の値（docs/notes/mobile-testflight.md）。`--set-github-secrets` で、`gh` コマンド経由で登録まで行う。
//
// 使い方:
//   node scripts/setup-ios-signing.mjs --key-id ABC123DEFG --issuer 69a6de80-xxxx-xxxx-xxxx-xxxxxxxxxxxx --p8 ~/Downloads/AuthKey_ABC123DEFG.p8
//   （任意）--bundle-id com.shiki01.hikae.mobile  --name Hikae  --out ./ios-signing  --set-github-secrets  --repo shiki-01/Hikae
//
// 注意: 秘密情報（.p12、.p8 の base64、パスワード）を --out のフォルダに書く。このフォルダはコミットしない（.gitignore 済み）。
// アプリの登録（App Store Connect の「マイ App」）は API では作れないため、画面で行う。
import { createPrivateKey, sign, randomBytes } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const DEFAULT_API = 'https://api.appstoreconnect.apple.com';
const DEFAULT_BUNDLE_ID = 'com.shiki01.hikae.mobile';

const b64url = (buf) => Buffer.from(buf).toString('base64url');

/** App Store Connect API 用の JWT（ES256）。有効期限は 19 分（上限は 20 分） */
export function createJwt({ keyId, issuer, p8, now = Math.floor(Date.now() / 1000) }) {
	const header = { alg: 'ES256', kid: keyId, typ: 'JWT' };
	const payload = { iss: issuer, iat: now, exp: now + 19 * 60, aud: 'appstoreconnect-v1' };
	const input = `${b64url(JSON.stringify(header))}.${b64url(JSON.stringify(payload))}`;
	const signature = sign('sha256', Buffer.from(input), {
		key: createPrivateKey(p8),
		dsaEncoding: 'ieee-p1363'
	});
	return `${input}.${b64url(signature)}`;
}

function openssl(args, options = {}) {
	return execFileSync('openssl', args, { stdio: ['ignore', 'pipe', 'pipe'], ...options });
}

/** 秘密鍵と CSR（証明書署名要求）を作る */
export function createCsr(dir, commonName) {
	const key = join(dir, 'distribution.key.pem');
	const csr = join(dir, 'distribution.csr.pem');
	openssl([
		'req',
		'-new',
		'-newkey',
		'rsa:2048',
		'-nodes',
		'-keyout',
		key,
		'-out',
		csr,
		'-subj',
		`/CN=${commonName}/C=JP`
	]);
	return { key, csr, csrPem: readFileSync(csr, 'utf8') };
}

/**
 * 証明書（DER）と秘密鍵から .p12 を作る。
 * macOS の `security import` が読める古い暗号方式（3DES + SHA1）を指定する
 * （OpenSSL 3 の既定の方式は、古い macOS で「MAC の検証に失敗」になることがある）
 */
export function createP12(dir, keyPath, certDerBase64, password) {
	const der = join(dir, 'distribution.cer');
	const pem = join(dir, 'distribution.cert.pem');
	const p12 = join(dir, 'distribution.p12');
	writeFileSync(der, Buffer.from(certDerBase64, 'base64'));
	openssl(['x509', '-inform', 'DER', '-in', der, '-out', pem]);
	openssl([
		'pkcs12',
		'-export',
		'-inkey',
		keyPath,
		'-in',
		pem,
		'-out',
		p12,
		'-keypbe',
		'PBE-SHA1-3DES',
		'-certpbe',
		'PBE-SHA1-3DES',
		'-macalg',
		'sha1',
		'-passout',
		`pass:${password}`
	]);
	return p12;
}

class AscError extends Error {
	constructor(status, body) {
		const detail =
			body?.errors?.map((e) => `${e.title ?? ''}: ${e.detail ?? ''}`).join(' / ') ?? '';
		super(`App Store Connect API が ${status} を返しました。${detail}`);
		this.status = status;
	}
}

function client(base, jwt) {
	const call = async (method, path, body) => {
		const response = await fetch(base + path, {
			method,
			headers: { Authorization: `Bearer ${jwt}`, 'Content-Type': 'application/json' },
			body: body ? JSON.stringify(body) : undefined
		});
		const text = await response.text();
		const json = text ? JSON.parse(text) : null;
		if (!response.ok) throw new AscError(response.status, json);
		return json;
	};
	return { get: (p) => call('GET', p), post: (p, b) => call('POST', p, b) };
}

/** App ID（Bundle ID）を探し、無ければ登録する。Team ID（seedId）も返す */
export async function ensureBundleId(api, identifier, name) {
	const found = await api.get(
		`/v1/bundleIds?filter[identifier]=${encodeURIComponent(identifier)}&filter[platform]=IOS`
	);
	const hit = found.data.find((d) => d.attributes.identifier === identifier);
	if (hit) return { id: hit.id, teamId: hit.attributes.seedId, created: false };
	const created = await api.post('/v1/bundleIds', {
		data: { type: 'bundleIds', attributes: { identifier, name, platform: 'IOS' } }
	});
	return { id: created.data.id, teamId: created.data.attributes.seedId, created: true };
}

export async function createCertificate(api, csrContent) {
	const res = await api.post('/v1/certificates', {
		data: { type: 'certificates', attributes: { certificateType: 'IOS_DISTRIBUTION', csrContent } }
	});
	return { id: res.data.id, content: res.data.attributes.certificateContent };
}

export async function createProfile(api, { name, bundleIdId, certificateId }) {
	const res = await api.post('/v1/profiles', {
		data: {
			type: 'profiles',
			attributes: { name, profileType: 'IOS_APP_STORE' },
			relationships: {
				bundleId: { data: { type: 'bundleIds', id: bundleIdId } },
				certificates: { data: [{ type: 'certificates', id: certificateId }] }
			}
		}
	});
	return res.data.attributes.profileContent;
}

export function parseArgs(argv) {
	const args = {
		out: './ios-signing',
		bundleId: DEFAULT_BUNDLE_ID,
		name: 'Hikae',
		api: DEFAULT_API
	};
	for (let i = 0; i < argv.length; i++) {
		const a = argv[i];
		const next = () => argv[++i];
		if (a === '--key-id') args.keyId = next();
		else if (a === '--issuer') args.issuer = next();
		else if (a === '--p8') args.p8 = next();
		else if (a === '--bundle-id') args.bundleId = next();
		else if (a === '--name') args.name = next();
		else if (a === '--out') args.out = next();
		else if (a === '--repo') args.repo = next();
		else if (a === '--api-base')
			args.api = next(); // テスト用
		else if (a === '--set-github-secrets') args.setSecrets = true;
		else throw new Error(`不明な引数: ${a}`);
	}
	for (const k of ['keyId', 'issuer', 'p8']) {
		if (!args[k])
			throw new Error(
				`--${k === 'keyId' ? 'key-id' : k} が必要です（使い方はファイルの先頭を見てください）`
			);
	}
	return args;
}

export async function run(args, log = console.log) {
	const p8 = readFileSync(args.p8, 'utf8');
	const jwt = createJwt({ keyId: args.keyId, issuer: args.issuer, p8 });
	const api = client(args.api, jwt);
	const work = mkdtempSync(join(tmpdir(), 'hikae-ios-signing-'));
	try {
		log('1/4 App ID を確認しています…');
		const bundle = await ensureBundleId(api, args.bundleId, args.name);
		log(
			bundle.created
				? `    登録しました（Team ID: ${bundle.teamId}）`
				: `    登録済みです（Team ID: ${bundle.teamId}）`
		);

		log('2/4 配布用の証明書を作っています…（Apple Distribution の証明書は 3 つまでです）');
		const { key, csrPem } = createCsr(work, `${args.name} Distribution`);
		const cert = await createCertificate(api, csrPem);

		log('3/4 プロビジョニングプロファイルを作っています…');
		const stamp = new Date().toISOString().slice(0, 10);
		const profile = await createProfile(api, {
			name: `${args.name} AppStore ${stamp} ${randomBytes(2).toString('hex')}`,
			bundleIdId: bundle.id,
			certificateId: cert.id
		});

		log('4/4 .p12 を作っています…');
		const password = randomBytes(18).toString('base64url');
		const p12 = createP12(work, key, cert.content, password);

		mkdirSync(args.out, { recursive: true, mode: 0o700 });
		const secrets = {
			APPLE_DEVELOPMENT_TEAM: bundle.teamId,
			IOS_CERTIFICATE: readFileSync(p12).toString('base64'),
			IOS_CERTIFICATE_PASSWORD: password,
			IOS_MOBILE_PROVISION: profile,
			APPLE_API_KEY_ID: args.keyId,
			APPLE_API_ISSUER: args.issuer,
			APPLE_API_KEY_P8: Buffer.from(p8).toString('base64')
		};
		for (const [name, value] of Object.entries(secrets)) {
			writeFileSync(join(args.out, `${name}.txt`), value, { mode: 0o600 });
		}
		log(
			`\n${Object.keys(secrets).length} 個の値を ${args.out}/ に書きました（GitHub のシークレットの名前と同じファイル名）。`
		);

		if (args.setSecrets) {
			const repoArgs = args.repo ? ['--repo', args.repo] : [];
			for (const [name, value] of Object.entries(secrets)) {
				execFileSync('gh', ['secret', 'set', name, ...repoArgs], {
					input: value,
					stdio: ['pipe', 'ignore', 'inherit']
				});
				log(`    GitHub に登録しました: ${name}`);
			}
		} else {
			log(
				'GitHub に登録するには、このコマンドに --set-github-secrets を付けて（gh にログイン済みで）もう一度実行するか、'
			);
			log(
				'Settings → Secrets and variables → Actions に、上のファイルの中身を貼り付けてください。'
			);
		}
		return secrets;
	} finally {
		rmSync(work, { recursive: true, force: true });
	}
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
	try {
		await run(parseArgs(process.argv.slice(2)));
		console.log(
			'\n残りの手作業: App Store Connect の「マイ App」でアプリを作る（Bundle ID は上の App ID を選ぶ）。'
		);
	} catch (error) {
		console.error(`\nエラー: ${error.message}`);
		process.exit(1);
	}
}
