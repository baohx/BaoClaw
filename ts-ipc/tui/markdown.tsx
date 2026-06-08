/**
 * Markdown 语法高亮解析器
 * 支持：代码块（带语言）、行内代码、粗体
 */
import React from 'react';
import { Text } from 'ink';

// 高亮色（硬编码，避免循环依赖 theme.ts）
const HL = {
  codeBg: '#1A1A1A',
  keyword: '#FF79C6',
  string: '#F1FA8C',
  comment: '#6272A4',
  fn: '#50FA7B',
  type: '#8BE9FD',
  number: '#BD93F9',
};

/**
 * 解析消息文本，返回 React 元素数组
 */
export function renderMarkdown(text: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  let i = 0;
  let key = 0;

  while (i < text.length) {
    // 代码块 ```language\n...\n```
    if (text.startsWith('```', i)) {
      const end = text.indexOf('\n', i);
      const lang = end !== -1 ? text.slice(i + 3, end).trim() : '';
      const blockStart = end !== -1 ? end + 1 : i + 3;
      const blockEnd = text.indexOf('```', blockStart);

      if (blockEnd !== -1) {
        const code = text.slice(blockStart, blockEnd);
        nodes.push(renderCodeBlock(code, lang, key++));
        i = blockEnd + 3;
        continue;
      }
    }

    // 行内代码 `code`
    if (text[i] === '`') {
      const end = text.indexOf('`', i + 1);
      if (end !== -1) {
        nodes.push(
          <Text key={key++} backgroundColor={HL.codeBg} color="yellow">
            {text.slice(i + 1, end)}
          </Text>,
        );
        i = end + 1;
        continue;
      }
    }

    // 粗体 **bold** 或 __bold__
    if ((text.startsWith('**', i) || text.startsWith('__', i)) && i + 2 < text.length) {
      const marker = text.slice(i, i + 2);
      const end = text.indexOf(marker, i + 2);
      if (end !== -1) {
        nodes.push(
          <Text key={key++} bold color="white">
            {text.slice(i + 2, end)}
          </Text>,
        );
        i = end + 2;
        continue;
      }
    }

    // 普通文本：累积到下一个特殊符号
    let j = i;
    while (j < text.length) {
      if (text[j] === '`' || text.startsWith('```', j) || text.startsWith('**', j) || text.startsWith('__', j)) {
        break;
      }
      j++;
    }

    if (j > i) {
      nodes.push(
        <Text key={key++} color="white">
          {text.slice(i, j)}
        </Text>,
      );
    }

    i = j;
  }

  return nodes;
}

/**
 * 渲染代码块（语法高亮）
 */
function renderCodeBlock(code: string, language: string, key: number): React.ReactNode {
  const keywords = new Set([
    'function', 'const', 'let', 'var', 'import', 'export', 'class', 'return',
    'if', 'else', 'for', 'while', 'switch', 'case', 'break', 'continue',
    'new', 'this', 'extends', 'implements', 'interface', 'type', 'enum',
    'async', 'await', 'try', 'catch', 'throw', 'default', 'from', 'as',
    'true', 'false', 'null', 'undefined', 'void',
  ]);

  const lines = code.split('\n');

  return (
    <React.Fragment key={key}>
      <Text color="gray" dimColor>{`┌─ ${language || 'code'}`}</Text>
      {lines.map((line, li) => (
        <Text key={li}>
          <Text color="gray" dimColor>{'│ '}</Text>
          {highlightLine(line, keywords)}
        </Text>
      ))}
      <Text color="gray" dimColor>└─</Text>
    </React.Fragment>
  );
}

function highlightLine(
  line: string,
  keywords: Set<string>,
): React.ReactNode {
  const parts: React.ReactNode[] = [];
  let i = 0;
  let k = 0;

  while (i < line.length) {
    // 注释
    if (line.startsWith('//', i)) {
      parts.push(<Text key={k++} color={HL.comment}>{line.slice(i)}</Text>);
      break;
    }

    // 字符串
    if (line[i] === '"' || line[i] === "'" || line[i] === '`') {
      const quote = line[i];
      let end = i + 1;
      while (end < line.length && line[end] !== quote) {
        if (line[end] === '\\') end++;
        end++;
      }
      parts.push(<Text key={k++} color={HL.string}>{line.slice(i, end + 1)}</Text>);
      i = end + 1;
      continue;
    }

    // 标识符/关键字
    if (/[a-zA-Z_$]/.test(line[i])) {
      let end = i;
      while (end < line.length && /[a-zA-Z0-9_$]/.test(line[end])) end++;
      const word = line.slice(i, end);
      const color = keywords.has(word)
        ? HL.keyword
        : /^[A-Z]/.test(word)
          ? HL.type
          : HL.fn;
      parts.push(<Text key={k++} color={color}>{word}</Text>);
      i = end;
      continue;
    }

    // 数字
    if (/[0-9]/.test(line[i])) {
      let end = i;
      while (end < line.length && /[0-9.]/.test(line[end])) end++;
      parts.push(<Text key={k++} color={HL.number}>{line.slice(i, end)}</Text>);
      i = end;
      continue;
    }

    // 其他字符
    parts.push(<Text key={k++} color="gray">{line[i]}</Text>);
    i++;
  }

  return <>{parts}</>;
}
