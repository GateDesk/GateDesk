/// The session panel: the window a headless client answers control requests in.
///
/// It plays the connection manager's part - it is started as `--gd-panel`, the process the
/// server spawns instead of `--cm` when the app was started without `--ui` - so how a request
/// reaches the screen is unchanged and only the drawing differs. What it drops is the tab bar
/// and the chat, both of which need somewhere else to be read, and what it adds is that it may
/// not hide itself: it is the only place a request can be answered, so a prompt that ends up
/// behind a window nobody can see is a prompt that times out.
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:window_manager/window_manager.dart';

import '../../common.dart';
import '../../consts.dart';
import '../../models/platform_model.dart';
import '../../models/server_model.dart';
import 'server_page.dart' show checkClickTime;

class SessionPanelPage extends StatefulWidget {
  const SessionPanelPage({Key? key}) : super(key: key);

  @override
  State<SessionPanelPage> createState() => _SessionPanelPageState();
}

class _SessionPanelPageState extends State<SessionPanelPage>
    with AutomaticKeepAliveClientMixin {
  _SessionPanelPageState() {
    gFFI.ffiModel.updateEventListener(gFFI.sessionId, "");
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return MultiProvider(
      providers: [
        ChangeNotifierProvider.value(value: gFFI.serverModel),
      ],
      child: Consumer<ServerModel>(
        builder: (context, serverModel, child) {
          final body = Scaffold(
            backgroundColor: Theme.of(context).colorScheme.background,
            body: _PanelBody(clients: serverModel.clients),
          );
          return isLinux
              ? buildVirtualWindowFrame(context, body)
              : workaroundWindowBorder(
                  context,
                  Container(
                    decoration: BoxDecoration(
                        border:
                            Border.all(color: MyTheme.color(context).border!)),
                    child: body,
                  ));
        },
      ),
    );
  }

  @override
  bool get wantKeepAlive => true;
}

/// The list of peers, and the one thing this window does that a normal one does not: come to
/// the front when a request arrives.
class _PanelBody extends StatefulWidget {
  final List<Client> clients;

  const _PanelBody({Key? key, required this.clients}) : super(key: key);

  @override
  State<_PanelBody> createState() => _PanelBodyState();
}

class _PanelBodyState extends State<_PanelBody> {
  /// Peers whose outstanding request this window has already come forward for. Kept so a peer
  /// that is answered and asks again does raise the window a second time, while the redraws that
  /// happen for any other reason do not.
  final Set<int> _raisedFor = {};

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _syncRaise());
  }

  @override
  void didUpdateWidget(covariant _PanelBody oldWidget) {
    super.didUpdateWidget(oldWidget);
    _syncRaise();
  }

  void _syncRaise() {
    if (!mounted) return;
    final pending =
        widget.clients.where((c) => c.pendingControl).map((c) => c.id).toSet();
    _raisedFor.removeWhere((id) => !pending.contains(id));
    for (final id in pending) {
      if (_raisedFor.add(id)) {
        _raiseWindow();
      }
    }
  }

  /// A request nobody can see is a request that times out. Nothing else is listening for it -
  /// the app was started without `--ui`, so there is no connection manager window to fall back
  /// on - which is why this window is always on top and is brought forward rather than left
  /// wherever the user last put it.
  void _raiseWindow() {
    windowManager.show();
    windowManager.focus();
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const _BrandHeader(),
        Expanded(
          child: widget.clients.isEmpty
              ? const _WaitingView()
              : ListView.builder(
                  padding: const EdgeInsets.fromLTRB(10, 10, 10, 12),
                  itemCount: widget.clients.length,
                  itemBuilder: (context, index) =>
                      _SessionCard(client: widget.clients[index]),
                ),
        ),
      ],
    );
  }
}

/// The window's title bar, in the brand's colours.
///
/// Drawn here rather than left to the platform because the window has no frame - see
/// `getHiddenTitleBarWindowOptions` in `main.dart` - so this is also what makes it draggable.
class _BrandHeader extends StatelessWidget {
  const _BrandHeader({Key? key}) : super(key: key);

  @override
  Widget build(BuildContext context) {
    return DragToMoveArea(
      child: Container(
        height: 54,
        padding: const EdgeInsets.symmetric(horizontal: 12),
        decoration: const BoxDecoration(
          gradient: LinearGradient(
            begin: Alignment.topRight,
            end: Alignment.bottomLeft,
            colors: [Color(0xff00bfe1), Color(0xff0071ff)],
          ),
        ),
        child: Row(
          children: [
            loadIcon(24),
            const SizedBox(width: 8),
            Expanded(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    bind.mainGetAppNameSync(),
                    style: const TextStyle(
                        color: Colors.white,
                        fontSize: 14,
                        fontWeight: FontWeight.bold),
                    overflow: TextOverflow.ellipsis,
                  ),
                  Text(
                    translate('Permissions'),
                    style:
                        const TextStyle(color: Colors.white70, fontSize: 11),
                    overflow: TextOverflow.ellipsis,
                  ),
                ],
              ),
            ),
            IconButton(
              onPressed: () => windowManager.close(),
              icon: const Icon(IconFont.close, color: Colors.white, size: 18),
              splashColor: Colors.transparent,
              hoverColor: Colors.transparent,
              tooltip: translate('Close'),
            ),
          ],
        ),
      ),
    );
  }
}

class _WaitingView extends StatelessWidget {
  const _WaitingView({Key? key}) : super(key: key);

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          const SizedBox(
            width: 26,
            height: 26,
            child: CircularProgressIndicator(strokeWidth: 2),
          ),
          const SizedBox(height: 18),
          Text(
            translate("Waiting for new connection ..."),
            style: TextStyle(color: MyTheme.darkGray, fontSize: 13),
          ),
          const SizedBox(height: 6),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 24),
            child: Text(
              translate("This window lets you decide what a remote user may do "
                  "on this device."),
              textAlign: TextAlign.center,
              style: TextStyle(color: MyTheme.darkGray, fontSize: 11),
            ),
          ),
        ],
      ),
    );
  }
}

/// One peer, everything that can be decided about it, and nothing else.
///
/// The order is the order the decisions become possible in: who is asking, whether to let them
/// in at all, whether to hand over mouse and keyboard, and what they may do once they have it.
class _SessionCard extends StatelessWidget {
  final Client client;

  const _SessionCard({Key? key, required this.client}) : super(key: key);

  bool get _connected => client.authorized && !client.disconnected;

  @override
  Widget build(BuildContext context) {
    final border = MyTheme.color(context).border!;
    return Container(
      margin: const EdgeInsets.only(bottom: 10),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: Theme.of(context).cardColor,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: border),
        boxShadow: [
          BoxShadow(
            color: Colors.black.withOpacity(0.06),
            blurRadius: 6,
            offset: const Offset(0, 2),
          ),
        ],
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _PeerRow(client: client, connected: _connected),
          // A peer that is not through the door yet has exactly one decision to make, and it is
          // not a permission: `sendLoginResponse` is what answers `Login`, and until it is called
          // the server keeps the connection waiting. Dismiss ends it the same way a refusal does.
          if (!client.authorized && !client.disconnected) ...[
            const SizedBox(height: 10),
            _LoginRequest(client: client),
          ],
          if (_connected && client.pendingControl) ...[
            const SizedBox(height: 10),
            _ControlRequest(client: client),
          ],
          if (_connected) ...[
            const SizedBox(height: 10),
            _PermissionList(client: client),
          ],
          const SizedBox(height: 10),
          _CardActions(client: client, connected: _connected),
        ],
      ),
    );
  }
}

class _PeerRow extends StatelessWidget {
  final Client client;
  final bool connected;

  const _PeerRow({Key? key, required this.client, required this.connected})
      : super(key: key);

  @override
  Widget build(BuildContext context) {
    final name = client.name.isEmpty ? 'NA' : client.name;
    final state = client.disconnected
        ? translate('Disconnected')
        : connected
            ? translate('Connected')
            : translate('Waiting');
    return Row(
      children: [
        buildAvatarWidget(
              avatar: client.avatar,
              size: 38,
              fallback: Container(
                width: 38,
                height: 38,
                alignment: Alignment.center,
                decoration: BoxDecoration(
                    color: str2color(name), shape: BoxShape.circle),
                child: Text(
                  name.substring(0, 1).toUpperCase(),
                  style: const TextStyle(
                      color: Colors.white,
                      fontSize: 16,
                      fontWeight: FontWeight.bold),
                ),
              ),
            ) ??
            const SizedBox(width: 38, height: 38),
        const SizedBox(width: 10),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                name,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style:
                    const TextStyle(fontSize: 14, fontWeight: FontWeight.w600),
              ),
              const SizedBox(height: 2),
              Text(
                _typeLabel(client) == null
                    ? client.peerId
                    : '${client.peerId} · ${_typeLabel(client)}',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: MyTheme.darkGray, fontSize: 11),
              ),
            ],
          ),
        ),
        _StatePill(text: state, on: connected),
      ],
    );
  }
}

/// What the peer is asking to do, in the words the rest of the app uses for it, or null for the
/// ordinary case of a remote desktop.
String? _typeLabel(Client client) {
  switch (client.type_()) {
    case ClientType.file:
      return translate('Transfer file');
    case ClientType.camera:
      return translate('View camera');
    case ClientType.terminal:
      return translate('Terminal');
    case ClientType.portForward:
      return '${translate('Port forwarding')}: ${client.portForward}';
    case ClientType.remote:
      return null;
  }
}

class _StatePill extends StatelessWidget {
  final String text;
  final bool on;

  const _StatePill({Key? key, required this.text, required this.on})
      : super(key: key);

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
      decoration: BoxDecoration(
        color: on ? MyTheme.accent.withOpacity(0.12) : Colors.grey.withOpacity(0.18),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Text(
        text,
        style: TextStyle(
          fontSize: 11,
          fontWeight: FontWeight.w600,
          color: on ? MyTheme.accent : MyTheme.darkGray,
        ),
      ),
    );
  }
}

/// A decision the peer is waiting on, drawn the same way for both kinds of request.
class _RequestBox extends StatelessWidget {
  final IconData icon;
  final String title;
  final Widget? content;
  final List<Widget> actions;

  const _RequestBox({
    Key? key,
    required this.icon,
    required this.title,
    this.content,
    required this.actions,
  }) : super(key: key);

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(10),
      decoration: BoxDecoration(
        color: MyTheme.accent.withOpacity(0.08),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: MyTheme.accent.withOpacity(0.35)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Icon(icon, size: 16, color: MyTheme.accent),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  title,
                  style: const TextStyle(
                      fontSize: 12, fontWeight: FontWeight.w600),
                ),
              ),
            ],
          ),
          if (content != null) ...[
            const SizedBox(height: 8),
            content!,
          ],
          const SizedBox(height: 10),
          Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: actions
                .map((w) => Padding(
                    padding: const EdgeInsets.only(left: 8), child: w))
                .toList(),
          ),
        ],
      ),
    );
  }
}

/// Letting a peer in at all. Nothing else is listening for the peer's `Login` when the app was
/// started without `--ui`, so this prompt is the only way the session can begin.
class _LoginRequest extends StatelessWidget {
  final Client client;

  const _LoginRequest({Key? key, required this.client}) : super(key: key);

  @override
  Widget build(BuildContext context) {
    final model = Provider.of<ServerModel>(context, listen: false);
    return _RequestBox(
      icon: Icons.login_rounded,
      title: '${translate('Request access to your device')}...',
      actions: [
        _PanelButton(
          text: translate('Dismiss'),
          onTap: () => checkClickTime(
              client.id, () => model.sendLoginResponse(client, false)),
        ),
        // With `approve-mode` on password the peer is already past the door by the time it
        // reaches this window, so there is nothing left to accept. Same rule as the connection
        // manager's `showAccept`.
        if (model.approveMode != 'password')
          _PanelButton(
            text: translate('Accept'),
            primary: true,
            onTap: () => checkClickTime(
                client.id, () => model.sendLoginResponse(client, true)),
          ),
      ],
    );
  }
}

/// The peer asking to drive this machine's mouse and keyboard.
///
/// Every session starts view-only - see `Connection::resolve_control_request` in
/// `src/server/connection.rs` - so this is a request, not a state: ignoring it is a decision the
/// server makes on the user's behalf once the countdown runs out, and the countdown is drawn from
/// the deadline that decision was measured from.
class _ControlRequest extends StatefulWidget {
  final Client client;

  const _ControlRequest({Key? key, required this.client}) : super(key: key);

  @override
  State<_ControlRequest> createState() => _ControlRequestState();
}

class _ControlRequestState extends State<_ControlRequest> {
  Timer? _ticker;

  @override
  void initState() {
    super.initState();
    _ticker = Timer.periodic(const Duration(seconds: 1), (_) {
      if (mounted) setState(() {});
    });
  }

  @override
  void dispose() {
    _ticker?.cancel();
    super.dispose();
  }

  /// Seconds left before the server answers this request itself. Null when the deadline is not
  /// known here - the request was already outstanding when this window opened, and only the
  /// server still counts - in which case the prompt waits without a number rather than inventing
  /// one. See [Client.controlDeadline].
  int? get _secondsLeft {
    final deadline = widget.client.controlDeadline;
    if (deadline == null) return null;
    final ms = deadline.difference(DateTime.now()).inMilliseconds;
    return ms <= 0 ? 0 : (ms / 1000).ceil();
  }

  @override
  Widget build(BuildContext context) {
    final left = _secondsLeft;
    final ratio = left == null
        ? 1.0
        : (left / kControlRequestTimeoutSeconds).clamp(0.0, 1.0);
    // The button that ends the wait is the one the peer wants pressed, so it is drawn on the
    // right; the refusal keeps the left, where the outline weight stops it from reading as the
    // default. Same order as the connection manager's own prompt.
    return _RequestBox(
      icon: Icons.pan_tool_alt_rounded,
      title: translate('A remote user requests to control your mouse and keyboard'),
      content: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          ClipRRect(
            borderRadius: BorderRadius.circular(3),
            child: LinearProgressIndicator(
              value: ratio,
              minHeight: 4,
              backgroundColor: MyTheme.accent.withOpacity(0.15),
            ),
          ),
          const SizedBox(height: 6),
          Text(
            left == null
                ? translate('Waiting')
                : '${translate('Timeout')}: ${left}s',
            style: TextStyle(color: MyTheme.darkGray, fontSize: 11),
          ),
        ],
      ),
      actions: [
        // A peer that already has the keyboard must not be able to press these: the click
        // is time-stamped first, like every control in the connection manager.
        _PanelButton(
          text: translate('Deny'),
          onTap: () => checkClickTime(widget.client.id,
              () => bind.cmRespondControlRequest(
                  connId: widget.client.id, accepted: false)),
        ),
        _PanelButton(
          text: translate('Allow'),
          primary: true,
          onTap: () => checkClickTime(widget.client.id,
              () => bind.cmRespondControlRequest(
                  connId: widget.client.id, accepted: true)),
        ),
      ],
    );
  }
}

/// What the peer may do, as a list of switches rather than a grid of icons.
///
/// The connection manager's grid works because there is room for a tooltip next to each icon;
/// this window is narrower and carries no chat, so each permission gets its own row and its own
/// label instead.
class _PermissionList extends StatefulWidget {
  final Client client;

  const _PermissionList({Key? key, required this.client}) : super(key: key);

  @override
  State<_PermissionList> createState() => _PermissionListState();
}

class _PermissionListState extends State<_PermissionList> {
  Client get client => widget.client;

  /// Whether the person at this machine may change these at all. The option exists for locked-down
  /// deployments, and the connection manager reads it the same way.
  bool get _canModify =>
      bind.mainGetBuildinOption(key: kOptionEnablePermChangeInAcceptWindow) !=
      'N';

  void _switch(String name, bool enabled, ValueChanged<bool> apply) {
    // The server is what really decides - it rolls `privacy_mode` back on its own when the
    // platform refuses - and the value it settled on arrives with the next client state. The
    // local flip is only so the row does not sit there disagreeing until then.
    bind.cmSwitchPermission(connId: client.id, name: name, enabled: enabled);
    setState(() => apply(enabled));
  }

  @override
  Widget build(BuildContext context) {
    final rows = <Widget>[];
    if (client.type_() == ClientType.camera) {
      rows.add(_PermissionTile(
        clientId: client.id,
        label: 'Enable audio',
        value: client.audio,
        canModify: _canModify,
        onChanged: (v) => _switch('audio', v, (v) => client.audio = v),
      ));
      rows.add(_PermissionTile(
        clientId: client.id,
        label: 'Enable recording session',
        value: client.recording,
        canModify: _canModify,
        onChanged: (v) => _switch('recording', v, (v) => client.recording = v),
      ));
    } else {
      rows.add(_PermissionTile(
        clientId: client.id,
        label: 'Enable keyboard/mouse',
        value: client.keyboard,
        canModify: _canModify,
        onChanged: (v) => _switch('keyboard', v, (v) => client.keyboard = v),
      ));
      rows.add(_PermissionTile(
        clientId: client.id,
        label: 'Enable clipboard',
        value: client.clipboard,
        canModify: _canModify,
        onChanged: (v) => _switch('clipboard', v, (v) => client.clipboard = v),
      ));
      rows.add(_PermissionTile(
        clientId: client.id,
        label: 'Enable audio',
        value: client.audio,
        canModify: _canModify,
        onChanged: (v) => _switch('audio', v, (v) => client.audio = v),
      ));
      rows.add(_PermissionTile(
        clientId: client.id,
        label: 'Enable file copy and paste',
        value: client.file,
        canModify: _canModify,
        onChanged: (v) => _switch('file', v, (v) => client.file = v),
      ));
      rows.add(_PermissionTile(
        clientId: client.id,
        label: 'Enable remote restart',
        value: client.restart,
        canModify: _canModify,
        onChanged: (v) => _switch('restart', v, (v) => client.restart = v),
      ));
      rows.add(_PermissionTile(
        clientId: client.id,
        label: 'Enable recording session',
        value: client.recording,
        canModify: _canModify,
        onChanged: (v) => _switch('recording', v, (v) => client.recording = v),
      ));
      // Blocking the local user's input is a Windows-only facility, and privacy mode needs a
      // platform implementation to switch to; neither is offered where it would do nothing.
      if (isWindows) {
        rows.add(_PermissionTile(
          clientId: client.id,
          label: 'Enable blocking user input',
          value: client.blockInput,
          canModify: _canModify,
          onChanged: (v) =>
              _switch('block_input', v, (v) => client.blockInput = v),
        ));
      }
      if (bind.mainSupportedPrivacyModeImpls() != '[]') {
        rows.add(_PermissionTile(
          clientId: client.id,
          label: 'Enable privacy mode',
          value: client.privacyMode,
          canModify: _canModify,
          onChanged: (v) =>
              _switch('privacy_mode', v, (v) => client.privacyMode = v),
        ));
      }
    }
    return Container(
      padding: const EdgeInsets.fromLTRB(10, 4, 6, 4),
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.background,
        borderRadius: BorderRadius.circular(10),
      ),
      child: Column(children: rows),
    );
  }
}

class _PermissionTile extends StatelessWidget {
  final int clientId;
  final String label;
  final bool value;
  final bool canModify;
  final ValueChanged<bool> onChanged;

  const _PermissionTile({
    Key? key,
    required this.clientId,
    required this.label,
    required this.value,
    required this.canModify,
    required this.onChanged,
  }) : super(key: key);

  @override
  Widget build(BuildContext context) {
    // The whole row is the target, not the thumb alone: this window is small and the switch is
    // the only control on it. The click is time-stamped first, like every other control in the
    // connection manager, so a click synthesised by the peer cannot flip a permission.
    void toggle(bool v) {
      if (!canModify) return;
      checkClickTime(clientId, () => onChanged(v));
    }

    return InkWell(
      onTap: canModify ? () => toggle(!value) : null,
      child: SizedBox(
        height: 32,
        child: Row(
          children: [
            Expanded(
              child: Text(
                translate(label),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  fontSize: 12,
                  color: canModify ? null : MyTheme.darkGray,
                ),
              ),
            ),
            // A stock switch is wider than a row this list can afford, and `Transform` would scale
            // the painting without shrinking the space it claims. `FittedBox` does both, so the
            // row stays 32 high and nothing overflows.
            SizedBox(
              width: 44,
              height: 26,
              child: FittedBox(
                fit: BoxFit.fill,
                child: Switch(
                  value: value,
                  onChanged: canModify ? toggle : null,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _CardActions extends StatelessWidget {
  final Client client;
  final bool connected;

  const _CardActions({Key? key, required this.client, required this.connected})
      : super(key: key);

  /// Drop a session that has already ended. Nothing else will ever remove it, so the panel would
  /// sit there on top of the desktop listing a peer that is long gone.
  Future<void> _close() async {
    await bind.cmRemoveDisconnectedConnection(connId: client.id);
    if (await bind.cmGetClientsLength() == 0) {
      windowManager.close();
    }
  }

  @override
  Widget build(BuildContext context) {
    if (client.disconnected) {
      return Row(
        children: [
          Expanded(
            child: _PanelButton(
              text: translate('Close'),
              onTap: () => checkClickTime(client.id, _close),
            ),
          ),
        ],
      );
    }
    if (connected) {
      return Row(
        children: [
          Expanded(
            child: _PanelButton(
              text: translate('Disconnect'),
              danger: true,
              onTap: () => checkClickTime(
                  client.id, () => bind.cmCloseConnection(connId: client.id)),
            ),
          ),
        ],
      );
    }
    // A peer that has not been let in yet is answered by the prompt above, not from here.
    return const SizedBox.shrink();
  }
}

class _PanelButton extends StatelessWidget {
  final String text;
  final VoidCallback? onTap;
  final bool primary;
  final bool danger;

  const _PanelButton({
    Key? key,
    required this.text,
    required this.onTap,
    this.primary = false,
    this.danger = false,
  }) : super(key: key);

  @override
  Widget build(BuildContext context) {
    final color = danger ? Colors.redAccent : MyTheme.accent;
    final enabled = onTap != null;
    final fg = primary ? Colors.white : null;
    return Container(
      height: 30,
      decoration: BoxDecoration(
        color: primary && enabled ? color : Colors.transparent,
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: enabled ? color : MyTheme.darkGray),
      ),
      child: InkWell(
        borderRadius: BorderRadius.circular(8),
        onTap: onTap,
        child: Center(
          child: Text(
            text,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(
              fontSize: 12,
              fontWeight: FontWeight.w600,
              color: fg ?? (enabled ? color : MyTheme.darkGray),
            ),
          ),
        ),
      ),
    );
  }
}

