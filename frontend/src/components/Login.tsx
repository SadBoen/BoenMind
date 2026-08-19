import { useState } from "react";
import { Alert, Button, Card, Form, Input, Typography } from "antd";
import { LockOutlined } from "@ant-design/icons";
import { rpc } from "../client";

interface Props {
  onAuthed: (token: string) => void;
}

export default function Login({ onAuthed }: Props) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // antd Form：htmlType=submit + onFinish → Enter 键自动提交（不拦回车）。
  const submit = async (values: { password: string }) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const v = await rpc<{ token: string }>("auth.login", { password: values.password });
      onAuthed(v.token);
    } catch (err) {
      setError((err as Error).message === "auth-required" ? "密码错误" : (err as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="login-wrap">
      <Card className="login-card">
        <Typography.Title level={3}>BoenMind</Typography.Title>
        <p className="sub">请输入密码以继续（默认 adminadmin，首次登录后请在设置中修改）</p>
        <Form onFinish={submit} layout="vertical">
          <Form.Item name="password" rules={[{ required: true, message: "请输入密码" }]}>
            <Input.Password prefix={<LockOutlined />} placeholder="密码" autoFocus />
          </Form.Item>
          <Form.Item style={{ marginBottom: 0 }}>
            <Button type="primary" htmlType="submit" block loading={busy}>
              登录
            </Button>
          </Form.Item>
        </Form>
        {error && (
          <Alert type="error" showIcon message={error} style={{ marginTop: 16 }} />
        )}
      </Card>
    </div>
  );
}
