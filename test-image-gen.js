#!/usr/bin/env node
// BaoClaw 图片生成测试脚本
// 用法: node test-image-gen.js
// 需要设置环境变量: ANTHROPIC_API_KEY (GLM API Key)

const https = require('https');
const fs = require('fs');
const path = require('path');

const API_KEY = process.env.ANTHROPIC_API_KEY;
if (!API_KEY) {
  console.error('请设置 ANTHROPIC_API_KEY 环境变量');
  process.exit(1);
}

const prompt = '一个精美的三层生日蛋糕，粉色奶油装饰，顶部插着点燃的彩色蜡烛，周围飘落彩色糖霜和花瓣，温馨的庆祝氛围，高细节，柔和的灯光';

const data = JSON.stringify({
  model: 'cogview-4-250304',
  prompt: prompt,
  size: '1024x1024'
});

console.log('🎂 正在生成生日蛋糕图片...\n');

const req = https.request({
  hostname: 'open.bigmodel.cn',
  path: '/api/paas/v4/images/generations',
  method: 'POST',
  headers: {
    'Authorization': 'Bearer ' + API_KEY,
    'Content-Type': 'application/json',
    'Content-Length': Buffer.byteLength(data)
  }
}, (res) => {
  let body = '';
  res.on('data', (c) => body += c);
  res.on('end', () => {
    try {
      const r = JSON.parse(body);
      if (r.data && r.data[0] && r.data[0].url) {
        const imgUrl = r.data[0].url;
        console.log('✅ 图片生成成功！');
        console.log('📎 URL: ' + imgUrl);
        console.log('\n正在下载到本地...');

        const outDir = '/tmp/baoclaw-images';
        if (!fs.existsSync(outDir)) fs.mkdirSync(outDir, { recursive: true });
        const outFile = path.join(outDir, 'birthday-cake.png');

        https.get(imgUrl, (imgRes) => {
          const ws = fs.createWriteStream(outFile);
          imgRes.pipe(ws);
          ws.on('finish', () => {
            const size = fs.statSync(outFile).size;
            console.log(`💾 已保存: ${outFile} (${(size/1024).toFixed(1)} KB)`);
          });
        });
      } else {
        console.error('❌ API 返回错误: ' + body);
      }
    } catch (e) {
      console.error('❌ 解析失败: ' + body);
    }
  });
});

req.on('error', (e) => console.error('❌ 请求失败: ' + e.message));
req.write(data);
req.end();
