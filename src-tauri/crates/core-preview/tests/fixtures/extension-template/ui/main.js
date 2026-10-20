// 表示拡張の受け取り例（設計書 11.7・プロトコル v1）。
// ホストから render（mode "view"）を受け取り、テキストだけを描く。それ以外は unsupported として返す。
// 権限は使わない（通信・ファイル・Tauri の呼び出しはできない）。

const PROTOCOL_VERSION = 1;
const notice = document.getElementById('notice');
const content = document.getElementById('content');

/** ホストへ送る。origin は opaque のため "*" を使う */
function send(message) {
  window.parent.postMessage({ v: PROTOCOL_VERSION, ...message }, '*');
}

function showNotice(text) {
  notice.textContent = text;
  notice.hidden = false;
  content.hidden = true;
}

window.addEventListener('message', (event) => {
  // 親（ホスト）以外からのメッセージは受け付けない
  if (event.source !== window.parent) return;
  const data = event.data;
  if (!data || data.v !== PROTOCOL_VERSION || data.type !== 'render' || data.mode !== 'view') return;

  const id = data.id;
  const file = data.file || {};
  if (file.state !== 'ok') {
    showNotice('このファイルは表示できません。');
    return;
  }
  if (data.content && data.content.kind === 'text') {
    notice.hidden = true;
    content.textContent = data.content.text;
    content.hidden = false;
    return;
  }
  // このサンプルが扱わない内容の種類は、対応しないと返す
  showNotice('この形式は、この表示拡張では表示できません。');
  send({ type: 'error', id, code: 'unsupported' });
});

// 読み込みの完了は 1 回だけ知らせる。ホストはこれを待ってから render を送る
send({ type: 'ready' });
