// Google Scholar Cookie Exporter
// 
// 使用方法:
// 1. 打开 Chrome 浏览器，访问 https://scholar.google.com
// 2. 完成任何 CAPTCHA 验证
// 3. 按 F12 打开开发者工具
// 4. 切换到 Console 标签
// 5. 粘贴以下代码并按回车
// 6. 复制输出的 JSON，保存到 ~/.gscholar_cookies.json

(function () {
    const cookies = document.cookie.split(';').map(c => {
        const [name, ...valueParts] = c.trim().split('=');
        return {
            name: name,
            value: valueParts.join('='),
            domain: ".google.com",
            path: "/",
            secure: true,
            http_only: false,
            expires: null
        };
    });

    const json = JSON.stringify(cookies, null, 2);
    console.log('=== Cookie JSON (复制以下内容) ===');
    console.log(json);
    console.log('=== 结束 ===');
    console.log('请将上面的 JSON 保存到文件: ~/.gscholar_cookies.json');

    // 也尝试复制到剪贴板
    try {
        navigator.clipboard.writeText(json).then(() => {
            console.log('✓ 已自动复制到剪贴板！');
        });
    } catch (e) {
        console.log('请手动复制上面的 JSON');
    }
})();
