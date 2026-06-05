/**
 * Book Configuration
 * 
 * Agent Harness 技术书籍配置文件
 * 定义书籍元数据、章节结构、构建选项等
 */

import type { BookConfig } from './src/types';

const config: BookConfig = {
  title: 'Agent Harness',
  subtitle: '构建 AI Agent 运行时框架',
  author: 'BaoClaw Team',
  version: '1.0.0',
  repository: 'https://github.com/baoclaw/baoclaw',
  
  chapters: [
    {
      id: '01-fundamentals',
      path: 'chapters/01-fundamentals/README.md',
      title: '基础部分',
    },
    {
      id: '02-core-implementation',
      path: 'chapters/02-core-implementation/README.md',
      title: '核心实现',
    },
    {
      id: '03-memory-context',
      path: 'chapters/03-memory-context/README.md',
      title: '记忆与上下文',
    },
    {
      id: '04-ipc-multiclient',
      path: 'chapters/04-ipc-multiclient/README.md',
      title: 'IPC 与多客户端',
    },
    {
      id: '05-production',
      path: 'chapters/05-production/README.md',
      title: '生产实践',
    },
    {
      id: '06-advanced-patterns',
      path: 'chapters/06-advanced-patterns/README.md',
      title: '高级模式',
    },
  ],
  
  build: {
    outputDir: 'dist',
    minify: true,
    serviceWorker: true,
    syntaxTheme: {
      light: 'github-light',
      dark: 'github-dark',
      languages: ['rust', 'typescript', 'bash', 'markdown'],
    },
  },
};

export default config;
