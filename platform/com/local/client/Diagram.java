package com.local.client;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.RectF;
import android.view.MotionEvent;
import android.view.View;
import org.json.JSONArray;
import org.json.JSONObject;

/** Android Canvas binding for bounded geometry computed by the Rust monitor. */
final class Diagram extends View {
    interface Action { void run(String event); }
    private JSONObject drawing; private final JSONObject palette;
    private final Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Action action;
    private float offset,scale=1f,downX,downY;
    Diagram(Context context,JSONObject drawing,JSONObject palette,Action action){
        super(context);this.drawing=drawing;this.palette=palette;this.action=action;
        setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_NO);
    }
    void update(JSONObject next){drawing=next;invalidate();}
    @Override protected void onMeasure(int widthSpec,int heightSpec){
        int width=MeasureSpec.getSize(widthSpec);
        float ratio=(float)drawing.optDouble("height",340)/1000f;
        setMeasuredDimension(width,Math.max(1,Math.round(width*ratio)));
    }
    private int color(String key){try{return Color.parseColor(palette.optString(key,"#A2BCC5"));}catch(Exception ignored){return Color.GRAY;}}
    @Override protected void onDraw(Canvas canvas){
        super.onDraw(canvas);scale=getWidth()/1000f;offset=0;canvas.save();canvas.scale(scale,scale);
        JSONArray shapes=drawing.optJSONArray("shapes");
        if(shapes!=null)for(int i=0;i<Math.min(2048,shapes.length());i++){
            JSONObject p=shapes.optJSONObject(i);if(p==null)continue;
            float x=(float)p.optDouble("x"),y=(float)p.optDouble("y"),x2=(float)p.optDouble("x2"),y2=(float)p.optDouble("y2");
            paint.setColor(color(p.optString("color")));paint.setStyle(Paint.Style.FILL);paint.setStrokeWidth((float)p.optDouble("width",2));paint.setStrokeCap(Paint.Cap.ROUND);
            switch(p.optString("kind")){
                case "line":canvas.drawLine(x,y,x2,y2,paint);break;
                case "rect":canvas.drawRoundRect(new RectF(x,y,x2,y2),10,10,paint);break;
                case "circle":canvas.drawCircle(x,y,(float)p.optDouble("radius"),paint);break;
                case "arc":paint.setStyle(Paint.Style.STROKE);canvas.drawArc(new RectF(x,y,x2,y2),(float)p.optDouble("start"),(float)p.optDouble("sweep"),false,paint);break;
                case "text":paint.setTextSize((float)p.optDouble("size",30));String align=p.optString("align");paint.setTextAlign(align.equals("center")?Paint.Align.CENTER:align.equals("right")?Paint.Align.RIGHT:Paint.Align.LEFT);canvas.drawText(p.optString("value"),x,y,paint);break;
            }
        }
        canvas.restore();
    }
    @Override public boolean onTouchEvent(MotionEvent event){
        if(event.getAction()==MotionEvent.ACTION_DOWN){downX=event.getX();downY=event.getY();return true;}
        if(event.getAction()==MotionEvent.ACTION_UP){
            if(Math.abs(downX-event.getX())+Math.abs(downY-event.getY())>24*getResources().getDisplayMetrics().density)return false;
            float x=(event.getX()-offset)/scale,y=event.getY()/scale;JSONArray shapes=drawing.optJSONArray("shapes");
            if(shapes!=null)for(int i=0;i<shapes.length();i++){
                JSONObject p=shapes.optJSONObject(i);if(p==null||p.optString("event").isEmpty())continue;
                if(x>=p.optDouble("x")&&x<=p.optDouble("x2")&&y>=p.optDouble("y")&&y<=p.optDouble("y2")){performClick();action.run(p.optString("event"));return true;}
            }
        }
        return super.onTouchEvent(event);
    }
    @Override public boolean performClick(){super.performClick();return true;}
}
