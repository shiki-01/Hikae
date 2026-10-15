// scripts/setup-ios-signing.mjs のテスト。App Store Connect の代わりに、手元の疑似サーバーを使う。
import test from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import { execFileSync } from 'node:child_process';
import { createPublicKey, generateKeyPairSync, verify } from 'node:crypto';
import { mkdtempSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createJwt, parseArgs, run } from './setup-ios-signing.mjs';

function testKey() {
	const { privateKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' });
	return privateKey.export({ type: 'pkcs8', format: 'pem' });
}

test('JWT は ES256 で署名され、公開鍵で検証できる', () => {
	const p8 = testKey();
	const jwt = createJwt({ keyId: 'KEYID12345', issuer: 'issuer-uuid', p8, now: 1_800_000_000 });
	const [h, p, s] = jwt.split('.');
	const header = JSON.parse(Buffer.from(h, 'base64url'));
	const payload = JSON.parse(Buffer.from(p, 'base64url'));
	assert.deepEqual(header, { alg: 'ES256', kid: 'KEYID12345', typ: 'JWT' });
	assert.equal(payload.iss, 'issuer-uuid');
	assert.equal(payload.aud, 'appstoreconnect-v1');
	assert.equal(payload.exp - payload.iat, 19 * 60);
	const publicKey = createPublicKey(p8);
	assert.ok(
		verify(
			'sha256',
			Buffer.from(`${h}.${p}`),
			{ key: publicKey, dsaEncoding: 'ieee-p1363' },
			Buffer.from(s, 'base64url')
		)
	);
});

test('引数の検査', () => {
	assert.throws(() => parseArgs([]), /--key-id/);
	assert.throws(() => parseArgs(['--key-id', 'a', '--issuer', 'b']), /--p8/);
	assert.throws(() => parseArgs(['--nope']), /不明な引数/);
	const a = parseArgs(['--key-id', 'a', '--issuer', 'b', '--p8', 'c']);
	assert.equal(a.bundleId, 'com.shiki01.hikae.mobile');
});

/** 疑似の App Store Connect。受けたリクエストを記録する */
function fakeAsc(workDir) {
	const requests = [];
	const caKey = join(workDir, 'ca.key');
	const caPem = join(workDir, 'ca.pem');
	execFileSync(
		'openssl',
		[
			'req',
			'-x509',
			'-newkey',
			'rsa:2048',
			'-nodes',
			'-keyout',
			caKey,
			'-out',
			caPem,
			'-subj',
			'/CN=Fake CA',
			'-days',
			'2'
		],
		{ stdio: 'ignore' }
	);
	let bundle = null;
	const server = http.createServer((req, res) => {
		let body = '';
		req.on('data', (c) => (body += c));
		req.on('end', () => {
			const json = body ? JSON.parse(body) : null;
			requests.push({ method: req.method, url: req.url, auth: req.headers.authorization, json });
			const send = (status, obj) => {
				res.writeHead(status, { 'Content-Type': 'application/json' });
				res.end(JSON.stringify(obj));
			};
			if (req.method === 'GET' && req.url.startsWith('/v1/bundleIds')) {
				return send(200, { data: bundle ? [bundle] : [] });
			}
			if (req.method === 'POST' && req.url === '/v1/bundleIds') {
				bundle = { id: 'BUNDLE1', attributes: { ...json.data.attributes, seedId: 'TEAM123456' } };
				return send(201, { data: bundle });
			}
			if (req.method === 'POST' && req.url === '/v1/certificates') {
				const csr = join(workDir, 'req.csr');
				const der = join(workDir, 'issued.cer');
				writeFileSync(csr, json.data.attributes.csrContent);
				execFileSync(
					'openssl',
					[
						'x509',
						'-req',
						'-in',
						csr,
						'-CA',
						caPem,
						'-CAkey',
						caKey,
						'-CAcreateserial',
						'-days',
						'1',
						'-outform',
						'DER',
						'-out',
						der
					],
					{ stdio: 'ignore' }
				);
				return send(201, {
					data: {
						id: 'CERT1',
						attributes: { certificateContent: readFileSync(der).toString('base64') }
					}
				});
			}
			if (req.method === 'POST' && req.url === '/v1/profiles') {
				return send(201, {
					data: {
						id: 'PROF1',
						attributes: { profileContent: Buffer.from('fake-profile').toString('base64') }
					}
				});
			}
			return send(404, { errors: [{ title: 'not found', detail: req.url }] });
		});
	});
	return new Promise((resolve) =>
		server.listen(0, '127.0.0.1', () =>
			resolve({ server, requests, url: `http://127.0.0.1:${server.address().port}` })
		)
	);
}

test('証明書・App ID・プロファイルを作り、7 個の値を書き出す', async () => {
	const dir = mkdtempSync(join(tmpdir(), 'hikae-signing-test-'));
	const p8Path = join(dir, 'AuthKey_KEYID12345.p8');
	writeFileSync(p8Path, testKey());
	const asc = await fakeAsc(dir);
	try {
		const out = join(dir, 'out');
		const secrets = await run(
			{
				keyId: 'KEYID12345',
				issuer: 'issuer-uuid',
				p8: p8Path,
				bundleId: 'com.example.app',
				name: 'Test',
				out,
				api: asc.url
			},
			() => {}
		);
		assert.deepEqual(Object.keys(secrets).sort(), [
			'APPLE_API_ISSUER',
			'APPLE_API_KEY_ID',
			'APPLE_API_KEY_P8',
			'APPLE_DEVELOPMENT_TEAM',
			'IOS_CERTIFICATE',
			'IOS_CERTIFICATE_PASSWORD',
			'IOS_MOBILE_PROVISION'
		]);
		assert.equal(secrets.APPLE_DEVELOPMENT_TEAM, 'TEAM123456');
		assert.equal(Buffer.from(secrets.IOS_MOBILE_PROVISION, 'base64').toString(), 'fake-profile');
		for (const name of Object.keys(secrets)) assert.ok(existsSync(join(out, `${name}.txt`)), name);

		// .p12 が OpenSSL で読める（パスワード・証明書・秘密鍵）
		const p12 = join(dir, 'check.p12');
		writeFileSync(p12, Buffer.from(secrets.IOS_CERTIFICATE, 'base64'));
		const info = execFileSync(
			'openssl',
			['pkcs12', '-in', p12, '-passin', `pass:${secrets.IOS_CERTIFICATE_PASSWORD}`, '-nodes'],
			{ stdio: ['ignore', 'pipe', 'pipe'] }
		).toString();
		assert.match(info, /BEGIN CERTIFICATE/);
		assert.match(info, /BEGIN (RSA |ENCRYPTED )?PRIVATE KEY/);
		// p8 は元のとおり
		assert.equal(
			Buffer.from(secrets.APPLE_API_KEY_P8, 'base64').toString(),
			readFileSync(p8Path, 'utf8')
		);

		// 送った内容
		const [list, createBundle, cert, profile] = asc.requests;
		assert.match(list.url, /filter\[identifier\]=com\.example\.app/);
		assert.equal(createBundle.json.data.attributes.platform, 'IOS');
		assert.equal(cert.json.data.attributes.certificateType, 'IOS_DISTRIBUTION');
		assert.match(cert.json.data.attributes.csrContent, /BEGIN CERTIFICATE REQUEST/);
		assert.equal(profile.json.data.attributes.profileType, 'IOS_APP_STORE');
		assert.deepEqual(profile.json.data.relationships.bundleId.data, {
			type: 'bundleIds',
			id: 'BUNDLE1'
		});
		assert.deepEqual(profile.json.data.relationships.certificates.data, [
			{ type: 'certificates', id: 'CERT1' }
		]);
		for (const r of asc.requests) assert.match(r.auth, /^Bearer [\w-]+\.[\w-]+\.[\w-]+$/);
	} finally {
		asc.server.close();
	}
});

test('API のエラーは、状態と理由を含むメッセージにする', async () => {
	const dir = mkdtempSync(join(tmpdir(), 'hikae-signing-test-'));
	const p8Path = join(dir, 'k.p8');
	writeFileSync(p8Path, testKey());
	const server = http.createServer((req, res) => {
		res.writeHead(403, { 'Content-Type': 'application/json' });
		res.end(
			JSON.stringify({
				errors: [{ title: 'Forbidden', detail: 'この API キーの権限が足りません' }]
			})
		);
	});
	await new Promise((r) => server.listen(0, '127.0.0.1', r));
	try {
		await assert.rejects(
			run(
				{
					keyId: 'K',
					issuer: 'I',
					p8: p8Path,
					bundleId: 'x.y',
					name: 'T',
					out: join(dir, 'o'),
					api: `http://127.0.0.1:${server.address().port}`
				},
				() => {}
			),
			/403.*権限が足りません/
		);
	} finally {
		server.close();
	}
});
