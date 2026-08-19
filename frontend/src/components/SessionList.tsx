import { useEffect, useState } from "react";
import { Button, Empty, List, Spin, Typography } from "antd";
import { CloseOutlined, PlusOutlined } from "@ant-design/icons";
import { rpc, SessionSummary } from "../client";
import { setCurrentSession, useCurrentSession } from "../sessionStore";

interface Props {
  onToggle: () => void;
  floating?: boolean;
}

export default function SessionList({ onToggle, floating }: Props) {
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const currentId = useCurrentSession();

  const refresh = async () => {
    setLoading(true);
    try {
      const v = await rpc<{ items: SessionSummary[] }>("session.list", {});
      setSessions(v.items ?? []);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { refresh(); }, []);

  const create = async () => {
    const v = await rpc<{ sessionId: string }>("session.create", {});
    setSessions((s) => [{ sessionId: v.sessionId, blank: true, running: false }, ...s]);
    setCurrentSession(v.sessionId);
  };

  return (
    <div className="session-list">
      <div className="session-list-header">
        <span>会话</span>
        <div className="session-list-actions">
          <Button size="small" type="text" icon={<PlusOutlined />} title="新建会话" onClick={create} />
          {floating && (
            <Button size="small" type="text" icon={<CloseOutlined />} title="关闭" onClick={onToggle} />
          )}
        </div>
      </div>
      <div className="session-list-body">
        {loading ? (
          <div className="muted"><Spin size="small" /> 加载中…</div>
        ) : sessions.length === 0 ? (
          <Empty description="暂无会话" image={Empty.PRESENTED_IMAGE_SIMPLE} />
        ) : (
          <List
            dataSource={sessions}
            split={false}
            renderItem={(s) => (
              <List.Item
                className={`session-item ${currentId === s.sessionId ? "active" : ""}`}
                onClick={() => setCurrentSession(s.sessionId)}
                style={{ padding: "8px 10px", cursor: "pointer" }}
              >
                <Typography.Text ellipsis className="session-title">
                  {s.blank ? "新会话" : s.sessionId.slice(0, 8)}
                </Typography.Text>
              </List.Item>
            )}
          />
        )}
      </div>
    </div>
  );
}
