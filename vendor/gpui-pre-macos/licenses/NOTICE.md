# 本地修改声明

gpui-pre-macos 0.3.9，来自 Zed revision 93d745668805029476e6d7fbb19652116fed51de，原作者与 Apache-2.0 许可保留于随附 LICENSE-APACHE。

LokiMetris 的本地兼容层在构建输出目录应用严格模板补丁（菜单锁、reopen、zoom、全屏失败通知与按需帧，登记见 build.rs）。原样上游文件不被修改。0.3.9 起可变字体补丁随上游 text_system 迁入 gpui-pre-apple。
