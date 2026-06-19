import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Window
import QtQuick.Effects

Window {
    id: win
    width: 900; height: 560
    flags: Qt.FramelessWindowHint | Qt.Window
    color: "transparent"
    visible: false
    title: "AppSwitcher"

    property color accent: backend.accent
    property color text1: "#EEF1F6"
    property color text2: "#9AA2B1"
    property string uiFont: "Segoe UI Variable Text"
    property string dispFont: "Bahnschrift"

    // ---------- reusable components ----------
    component Card: Rectangle {
        radius: 20
        color: Qt.rgba(1, 1, 1, 0.04)
        border.color: Qt.rgba(1, 1, 1, 0.06)
        border.width: 1
    }

    component Section: Text {
        color: win.text2; font.family: win.dispFont; font.pixelSize: 12
        font.letterSpacing: 2.5; font.bold: true
    }
    component Label2: Text { color: win.text2; font.family: win.uiFont; font.pixelSize: 13 }

    component IOSSwitch: Item {
        id: sw
        property bool checked: false
        signal toggled(bool v)
        implicitWidth: 50; implicitHeight: 30
        Rectangle {
            anchors.fill: parent; radius: height / 2
            color: sw.checked ? win.accent : "#3A3F4B"
            Behavior on color { ColorAnimation { duration: 180 } }
            Rectangle {
                width: 24; height: 24; radius: 12; color: "white"; y: 3
                x: sw.checked ? parent.width - 27 : 3
                Behavior on x { NumberAnimation { duration: 200; easing.type: Easing.OutBack } }
            }
        }
        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor
            onClicked: { sw.checked = !sw.checked; sw.toggled(sw.checked) } }
    }

    component Pill: Rectangle {
        property alias label: t.text
        property bool primary: false
        signal clicked()
        implicitWidth: t.implicitWidth + 34; implicitHeight: 40; radius: 13
        color: primary ? win.accent : Qt.rgba(1, 1, 1, 0.06)
        border.color: primary ? "transparent" : Qt.rgba(1, 1, 1, 0.08)
        scale: ma.pressed ? 0.96 : 1.0
        Behavior on scale { NumberAnimation { duration: 90 } }
        Behavior on color { ColorAnimation { duration: 150 } }
        Text { id: t; anchors.centerIn: parent; font.family: win.uiFont
            font.pixelSize: 14; font.bold: parent.primary
            color: parent.primary ? "#08121d" : win.text1 }
        MouseArea { id: ma; anchors.fill: parent; hoverEnabled: true
            cursorShape: Qt.PointingHandCursor; onClicked: parent.clicked() }
    }

    component Combo: ComboBox {
        id: cb
        font.family: win.uiFont; font.pixelSize: 14; implicitHeight: 42
        contentItem: Text { leftPadding: 16; verticalAlignment: Text.AlignVCenter
            text: cb.displayText; color: win.text1; font: cb.font
            textFormat: Text.PlainText
            // capitalize first letter
            Component.onCompleted: {} }
        background: Rectangle { radius: 13; color: Qt.rgba(1, 1, 1, 0.05)
            border.color: cb.activeFocus ? win.accent : Qt.rgba(1, 1, 1, 0.08); border.width: 1 }
        indicator: Text { x: cb.width - 26; y: (cb.height - height) / 2; text: "▾"; color: win.text2 }
        delegate: ItemDelegate { width: cb.width
            contentItem: Text { text: modelData; color: win.text1
                font.family: win.uiFont; font.pixelSize: 14; leftPadding: 10
                verticalAlignment: Text.AlignVCenter }
            background: Rectangle { radius: 9
                color: hovered ? Qt.rgba(win.accent.r, win.accent.g, win.accent.b, 0.18) : "transparent" } }
        popup.background: Rectangle { radius: 14; color: "#1E222C"; border.color: Qt.rgba(1,1,1,0.1) }
    }

    component Knob: ColumnLayout {
        property string title: ""
        property alias from: s.from
        property alias to: s.to
        property alias value: s.value
        property alias stepSize: s.stepSize
        property string suffix: ""
        signal moved()
        spacing: 5
        RowLayout {
            Label2 { text: title; Layout.fillWidth: true }
            Text { color: win.accent; font.family: win.dispFont; font.pixelSize: 15; font.bold: true
                text: (s.stepSize >= 1 ? Math.round(s.value) : s.value.toFixed(2)) + suffix }
        }
        Slider { id: s; Layout.fillWidth: true; onMoved: parent.moved()
            background: Rectangle { x: s.leftPadding; y: s.topPadding + s.availableHeight/2 - 3
                width: s.availableWidth; height: 6; radius: 3; color: Qt.rgba(1,1,1,0.10)
                Rectangle { width: s.visualPosition * parent.width; height: parent.height; radius: 3; color: win.accent } }
            handle: Rectangle { x: s.leftPadding + s.visualPosition * (s.availableWidth - width)
                y: s.topPadding + s.availableHeight/2 - height/2
                width: 18; height: 18; radius: 9; color: "white"; border.color: win.accent; border.width: 2
                scale: s.pressed ? 1.18 : 1.0; Behavior on scale { NumberAnimation { duration: 100 } } } }
    }

    // ---------- body ----------
    Rectangle {
        id: root
        anchors.fill: parent; anchors.margins: 14; radius: 26
        border.color: Qt.rgba(1, 1, 1, 0.08); border.width: 1
        gradient: Gradient {
            GradientStop { position: 0.0; color: "#1C2029" }
            GradientStop { position: 1.0; color: "#111319" }
        }
        layer.enabled: true
        layer.effect: MultiEffect { shadowEnabled: true; shadowColor: "#000000"
            shadowBlur: 1.0; shadowVerticalOffset: 10; shadowOpacity: 0.55 }

        // accent glow
        Rectangle { width: 340; height: 340; radius: 170; x: -60; y: -120
            color: win.accent; opacity: 0.13
            layer.enabled: true; layer.effect: MultiEffect { blurEnabled: true; blur: 1.0; blurMax: 80 } }

        ColumnLayout {
            anchors.fill: parent; anchors.margins: 26; spacing: 18

            // header (drag)
            Item {
                Layout.fillWidth: true; implicitHeight: 40
                MouseArea { anchors.fill: parent; onPressed: win.startSystemMove() }
                RowLayout {
                    anchors.fill: parent; spacing: 12
                    Rectangle { width: 34; height: 34; radius: 10; color: Qt.rgba(1,1,1,0.06)
                        Rectangle { width: 14; height: 9; radius: 2; color: win.accent; x: 7; y: 9 }
                        Rectangle { width: 14; height: 9; radius: 2; color: "#8A94A6"; x: 13; y: 16 } }
                    Text { text: "AppSwitcher"; color: win.text1; font.family: win.dispFont
                        font.pixelSize: 24; font.bold: true; Layout.fillWidth: true }
                    Rectangle { width: 32; height: 32; radius: 16
                        color: closeMa.containsMouse ? Qt.rgba(1,1,1,0.12) : "transparent"
                        Text { anchors.centerIn: parent; text: "✕"; color: win.text2; font.pixelSize: 14 }
                        MouseArea { id: closeMa; anchors.fill: parent; hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor; onClicked: win.visible = false } }
                }
            }

            // warning bar (full width)
            Card {
                Layout.fillWidth: true; implicitHeight: 58
                visible: backend.threeFingerWarning
                color: Qt.rgba(1, 0.75, 0.2, 0.12); border.color: Qt.rgba(1, 0.75, 0.2, 0.32)
                RowLayout {
                    anchors.fill: parent; anchors.leftMargin: 18; anchors.rightMargin: 14; spacing: 14
                    Text { text: "⚠"; font.pixelSize: 20; color: "#FFD479" }
                    ColumnLayout { Layout.fillWidth: true; spacing: 1
                        Text { text: "Windows 3-finger swipes are still on"; color: "#FFD479"
                            font.family: win.uiFont; font.pixelSize: 14; font.bold: true }
                        Label2 { text: "Set Touchpad → Three-finger gestures → Swipes to \"Nothing\"." } }
                    Pill { label: "Open Touchpad"; onClicked: backend.openTouchpadSettings() }
                    Pill { label: "Dismiss"; onClicked: backend.dismissWarning() }
                }
            }

            // two columns
            RowLayout {
                Layout.fillWidth: true; Layout.fillHeight: true; spacing: 18

                // LEFT — Look
                Card {
                    Layout.fillWidth: true; Layout.fillHeight: true
                    ColumnLayout {
                        anchors.fill: parent; anchors.margins: 20; spacing: 14
                        Section { text: "LOOK" }
                        Label2 { text: "Switch animation" }
                        Combo { Layout.fillWidth: true; model: backend.animations
                            Component.onCompleted: currentIndex = model.indexOf(backend.animation)
                            onActivated: backend.animation = model[currentIndex] }
                        Label2 { text: "Picker layout" }
                        Combo { Layout.fillWidth: true; model: backend.layoutsList
                            Component.onCompleted: currentIndex = model.indexOf(backend.layout)
                            onActivated: backend.layout = model[currentIndex] }
                        Item { Layout.fillHeight: true }
                        RowLayout { Layout.fillWidth: true; spacing: 10
                            Pill { label: "Preview layout"; Layout.fillWidth: true; onClicked: backend.previewLayout() }
                            Pill { label: "Preview anim"; Layout.fillWidth: true; onClicked: backend.previewAnim() } }
                    }
                }

                // RIGHT — Feel
                Card {
                    Layout.fillWidth: true; Layout.fillHeight: true
                    ColumnLayout {
                        anchors.fill: parent; anchors.margins: 20; spacing: 16
                        Section { text: "FEEL" }
                        Knob { Layout.fillWidth: true; title: "Animation speed"; suffix: " ms"
                            from: 80; to: 600; stepSize: 10; value: backend.duration
                            onMoved: backend.duration = value }
                        Knob { Layout.fillWidth: true; title: "Dock magnify"; suffix: "x"
                            from: 1.2; to: 2.6; stepSize: 0.05; value: backend.dockMag
                            onMoved: backend.dockMag = value }
                        Knob { Layout.fillWidth: true; title: "Sensitivity"
                            from: 120; to: 400; stepSize: 5; value: backend.sensitivity
                            onMoved: backend.sensitivity = value }
                        Item { Layout.fillHeight: true }
                        RowLayout {
                            Layout.fillWidth: true; spacing: 8
                            Section { text: "ACCENT"; Layout.alignment: Qt.AlignVCenter }
                            Item { width: 6 }
                            Repeater {
                                model: ["#96CDFF", "#7C9CFF", "#B98CFF", "#FF8CC6", "#5BE0B0", "#FFD479", "#FF8A65"]
                                Rectangle { width: 24; height: 24; radius: 12; color: modelData
                                    border.width: win.accent.toString().toLowerCase() === modelData.toLowerCase() ? 3 : 0
                                    border.color: "white"
                                    scale: sm.pressed ? 0.88 : 1.0; Behavior on scale { NumberAnimation { duration: 90 } }
                                    MouseArea { id: sm; anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                                        onClicked: backend.accent = modelData } }
                            }
                        }
                    }
                }
            }

            // bottom bar
            RowLayout {
                Layout.fillWidth: true; spacing: 14
                Label2 { text: "Replace Alt+Tab"; color: win.text1; font.pixelSize: 14 }
                IOSSwitch { checked: backend.altTab; onToggled: backend.altTab = v }
                Rectangle { width: 1; height: 22; color: Qt.rgba(1,1,1,0.12) }
                Label2 { text: "Start with Windows"; color: win.text1; font.pixelSize: 14 }
                IOSSwitch { checked: backend.autostart; onToggled: backend.autostart = v }
                Item { Layout.fillWidth: true }
                Pill { label: "Save"; primary: true; implicitWidth: 130; implicitHeight: 44
                    onClicked: { backend.save(); win.visible = false } }
            }
        }
    }
}
