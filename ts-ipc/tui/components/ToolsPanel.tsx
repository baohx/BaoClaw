/**
 * 工具面板组件
 */
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';
import { colors, zen, toolIcons } from '../theme.js';
import type { IpcClientImpl } from '../ipc.js';

interface ToolsPanelProps {
  ipc: IpcClientImpl | null;
  onClose: () => void;
}

interface ToolInfo {
  name: string;
  description?: string;
  type: string;
}

interface McpServerInfo {
  name: string;
  command?: string;
  url?: string;
  disabled: boolean;
  server_type: string;
}

interface SkillInfo {
  name: string;
  description?: string;
  source: string;
}

export function ToolsPanel({ ipc, onClose }: ToolsPanelProps) {
  const [tab, setTab] = useState<'tools' | 'mcp' | 'skills'>('tools');
  const [tools, setTools] = useState<ToolInfo[]>([]);
  const [mcpServers, setMcpServers] = useState<McpServerInfo[]>([]);
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const [loading, setLoading] = useState(true);
  
  useEffect(() => {
    async function loadData() {
      if (!ipc) return;
      
      try {
        const [toolsResult, mcpResult, skillsResult] = await Promise.all([
          ipc.request<{ tools: ToolInfo[]; count: number }>('listTools'),
          ipc.request<{ servers: McpServerInfo[]; count: number }>('listMcpServers'),
          ipc.request<{ skills: SkillInfo[]; count: number }>('listSkills'),
        ]);
        
        setTools(toolsResult.tools);
        setMcpServers(mcpResult.servers);
        setSkills(skillsResult.skills);
      } catch (error) {
        console.error('Failed to load tools:', error);
      }
      
      setLoading(false);
    }
    
    loadData();
  }, [ipc]);
  
  return (
    <Box 
      flexDirection="column"
      borderStyle="round"
      borderColor="yellow"
      paddingX={2}
      paddingY={1}
    >
      {/* 标签栏 */}
      <Box flexDirection="row" gap={2} marginBottom={1}>
        <Tab label="Tools" active={tab === 'tools'} count={tools.length} onClick={() => setTab('tools')} />
        <Tab label="MCP" active={tab === 'mcp'} count={mcpServers.length} onClick={() => setTab('mcp')} />
        <Tab label="Skills" active={tab === 'skills'} count={skills.length} onClick={() => setTab('skills')} />
      </Box>
      
      {/* 内容 */}
      <Box flexDirection="column" marginTop={1}>
        {loading ? (
          <Text color="gray">Loading...</Text>
        ) : (
          <>
            {tab === 'tools' && <ToolsList tools={tools} />}
            {tab === 'mcp' && <McpList servers={mcpServers} />}
            {tab === 'skills' && <SkillsList skills={skills} />}
          </>
        )}
      </Box>
      
      {/* 底部提示 */}
      <Box marginTop={1}>
        <Text color="gray">Press ESC to close</Text>
      </Box>
    </Box>
  );
}

interface TabProps {
  label: string;
  active: boolean;
  count: number;
  onClick: () => void;
}

function Tab({ label, active, count }: TabProps) {
  return (
    <Box>
      <Text color={active ? 'yellow' : 'gray'} bold={active}>
        {label}
      </Text>
      <Text color="gray"> ({count})</Text>
    </Box>
  );
}

function ToolsList({ tools }: { tools: ToolInfo[] }) {
  const grouped = tools.reduce((acc, tool) => {
    const type = tool.type || 'other';
    if (!acc[type]) acc[type] = [];
    acc[type].push(tool);
    return acc;
  }, {} as Record<string, ToolInfo[]>);
  
  return (
    <Box flexDirection="column">
      {Object.entries(grouped).map(([type, typeTools]) => (
        <Box key={type} flexDirection="column" marginBottom={1}>
          <Text color="cyan" bold>{type.toUpperCase()} ({typeTools.length})</Text>
          {typeTools.map((tool) => (
            <Box key={tool.name} flexDirection="row" paddingLeft={2}>
              <Text color="magenta">{toolIcons[tool.name] || '◆'}</Text>
              <Text color="white">{' '}{tool.name}</Text>
              {tool.description && (
                <Text color="gray">{'  '}{tool.description.slice(0, 40)}</Text>
              )}
            </Box>
          ))}
        </Box>
      ))}
    </Box>
  );
}

function McpList({ servers }: { servers: McpServerInfo[] }) {
  if (servers.length === 0) {
    return <Text color="gray">No MCP servers. Add to ~/.baoclaw/mcp.json</Text>;
  }
  
  return (
    <Box flexDirection="column">
      {servers.map((server) => (
        <Box key={server.name} flexDirection="column" marginBottom={1}>
          <Box flexDirection="row">
            <Text color={server.disabled ? 'red' : 'green'}>{server.disabled ? '○' : '●'}</Text>
            <Text color="white" bold>{' '}{server.name}</Text>
          </Box>
          <Box paddingLeft={3}>
            <Text color="gray">{server.command || server.url || server.server_type}</Text>
          </Box>
        </Box>
      ))}
    </Box>
  );
}

function SkillsList({ skills }: { skills: SkillInfo[] }) {
  if (skills.length === 0) {
    return <Text color="gray">No skills. Add to ~/.baoclaw/skills/</Text>;
  }
  
  return (
    <Box flexDirection="column">
      {skills.map((skill) => (
        <Box key={skill.name} flexDirection="column" marginBottom={1}>
          <Text color="white" bold>◆ {skill.name}</Text>
          {skill.description && <Text color="gray">  {skill.description}</Text>}
          <Text color="gray">  [{skill.source}]</Text>
        </Box>
      ))}
    </Box>
  );
}
